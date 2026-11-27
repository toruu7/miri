#![windows_subsystem = "windows"]

mod capture_stage;
mod config_store;
mod logging;
mod monitor;
mod pipe_server;
mod raw_input;
mod shell;
mod surface;

use capture_stage::CaptureStageWindow;
use config_store::{app_data_dir, reconcile_autostart, ConfigStore, ConfigWatcher};
use logging::{init_logging, log_error, log_info, log_warn};
use monitor::{
    current_process_stats, enumerate_monitors, evaluate_foreground, is_on_battery,
    monitor_work_area_for_anchor, monitor_work_area_for_hwnd, monitor_work_area_for_rect,
    set_eco_qos,
};
use pet_core::config::{anchor_to_work_area_pos, work_area_pos_to_norm};
use pet_core::{
    builtin_presets, clamp_rect_to_work_area, resolve_profile, BackdropMode, CatProfile,
    DesignerToOverlay, DiagnosticsSnapshot, OverlayToDesigner, PacingStage, PetSim, PoseOverride,
    PrivacySource, RectI32, Vec2, BASE_CANVAS_H, BASE_CANVAS_W, MAX_CANVAS_EDGE_PX, MAX_SIM_DT,
    PROTOCOL_VERSION,
};
use pet_render::{RenderOptions, Renderer};
use pipe_server::{PipeEvent, PipeSession};
use raw_input::{InputMailbox, RawInputWorker};
use shell::{
    copy_text_to_clipboard, HelperShell, CMD_COPY_DIAG, CMD_MONITOR_BASE, CMD_OPEN_CONFIG_DIR,
    CMD_OPEN_DESIGNER, CMD_PRESET_BASE, CMD_QUIT, CMD_RESET_POS, CMD_SCALE_LARGE, CMD_SCALE_MINI,
    CMD_SCALE_NORMAL, CMD_SCALE_SMALL, CMD_SNAP_BL, CMD_SNAP_BR, CMD_SNAP_TL, CMD_SNAP_TR,
    CMD_TOGGLE_AUTOSTART, CMD_TOGGLE_CAPTURE_STAGE, CMD_TOGGLE_LOCK, CMD_TOGGLE_POPUPS,
    CMD_TOGGLE_PRIVACY, CMD_TOGGLE_SHOWCASE, CMD_USER_PROFILE_BASE, HOTKEY_ID_CLICK_THROUGH,
    HOTKEY_ID_DESIGNER, HOTKEY_ID_PRIVACY, WM_APP_SUMMON_ESCAPE, WM_APP_TRAY,
};
use std::cell::RefCell;
use std::mem::size_of;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use surface::LayeredSurface;
use windows::core::{GUID, PCWSTR, w};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT,
    POINT, RECT, WPARAM,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::Power::{
    POWERBROADCAST_SETTING, RegisterPowerSettingNotification, UnregisterPowerSettingNotification,
};
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, ProcessIdToSessionId, WTSRegisterSessionNotification,
    WTSUnRegisterSessionNotification,
};
use windows::Win32::System::Threading::{
    CREATE_WAITABLE_TIMER_HIGH_RESOLUTION, CancelWaitableTimer, CreateEventW, CreateMutexW,
    CreateWaitableTimerExW, GetCurrentProcessId, ResetEvent, SetWaitableTimer, TIMER_ALL_ACCESS,
};
use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForWindow, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
const WM_MOUSELEAVE: u32 = 0x02A3;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, EVENT_SYSTEM_FOREGROUND,
    EVENT_SYSTEM_MINIMIZEEND, EVENT_SYSTEM_MOVESIZEEND, FindWindowW, GWL_EXSTYLE, GetCursorPos,
    GetWindowLongPtrW, GetWindowRect, HWND_TOPMOST, IDC_ARROW, IDC_HAND, LoadCursorW,
    MA_NOACTIVATE, MSG, MWMO_INPUTAVAILABLE, MsgWaitForMultipleObjectsEx, PM_REMOVE,
    PBT_APMSUSPEND, PBT_APMRESUMEAUTOMATIC, PBT_POWERSETTINGCHANGE, PeekMessageW, PostMessageW,
    QS_ALLINPUT, RegisterClassExW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SW_HIDE, SW_SHOWNOACTIVATE, SetCursor, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_CAPTURECHANGED,
    WM_CANCELMODE, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_HOTKEY, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_POWERBROADCAST, WM_RBUTTONUP, WM_WTSSESSION_CHANGE,
    WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
};

const GUID_CONSOLE_DISPLAY_STATE: GUID = GUID::from_u128(0x6fe69556_704a_47a0_8f24_c28d936fda47);

static GLOBAL_FG_DIRTY: AtomicBool = AtomicBool::new(true);
static GLOBAL_QUIT: AtomicBool = AtomicBool::new(false);

struct DragState {
    active: bool,
    grab_u: f32,
    grab_v: f32,
    win_w: i32,
    win_h: i32,
}

#[derive(Default)]
struct PendingUiActions {
    open_context_menu: bool,
    summon_escape: bool,
    toggle_lock: bool,
    toggle_privacy: bool,
    open_designer: bool,
    taskbar_recreated: bool,
    display_changed: bool,
    dpi_changed: Option<(u32, RECT)>,
    session_locked: Option<bool>,
    display_off: Option<bool>,
    hover_point_px: Option<(f32, f32)>,
    pointer_left: bool,
    drag_finished: bool,
    current_pos: Option<(i32, i32)>,
}

thread_local! {
    static UI_ACTIONS: RefCell<PendingUiActions> = RefCell::new(PendingUiActions::default());
    static DRAG_STATE: RefCell<DragState> = const { RefCell::new(DragState { active: false, grab_u: 0.5, grab_v: 0.5, win_w: 360, win_h: 300 }) };
    static HEAD_RECT_PX: RefCell<pet_core::HitRect> = RefCell::new(pet_core::HitRect::default());
    static CLICK_THROUGH_LOCKED: RefCell<bool> = const { RefCell::new(false) };
    static TRACKING_LEAVE: RefCell<bool> = const { RefCell::new(false) };
    static TASKBAR_MSG_ID: RefCell<u32> = const { RefCell::new(0) };
    static OVERLAY_HWND_TLS: RefCell<HWND> = RefCell::new(HWND::default());
}

fn compute_canvas_px(scale: f32, dpi: u32) -> (u32, u32) {
    let base_w = (BASE_CANVAS_W * scale).round() as i32;
    let base_h = (BASE_CANVAS_H * scale).round() as i32;
    let mut pw = ((base_w as i64 * dpi as i64 + 48) / 96).max(96) as u32;
    let mut ph = ((base_h as i64 * dpi as i64 + 48) / 96).max(80) as u32;
    let longest = pw.max(ph);
    if longest > MAX_CANVAS_EDGE_PX {
        let ratio = MAX_CANVAS_EDGE_PX as f32 / longest as f32;
        pw = ((pw as f32 * ratio).round() as u32).max(96);
        ph = ((ph as f32 * ratio).round() as u32).max(80);
    }
    (pw, ph)
}

fn apply_click_through_style(hwnd: HWND, locked: bool) {
    CLICK_THROUGH_LOCKED.with(|c| *c.borrow_mut() = locked);
    unsafe {
        let cur = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let next = if locked {
            cur | WS_EX_LAYERED.0 | WS_EX_TRANSPARENT.0
        } else {
            (cur | WS_EX_LAYERED.0) & !WS_EX_TRANSPARENT.0
        };
        if next != cur {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next as isize);
            let _ = SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
    }
}

fn qpc_now() -> i64 {
    let mut v = 0i64;
    unsafe {
        let _ = QueryPerformanceCounter(&mut v);
    }
    v
}

fn qpc_freq() -> f64 {
    let mut v = 1i64;
    unsafe {
        let _ = QueryPerformanceFrequency(&mut v);
    }
    v.max(1) as f64
}

fn main() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
    init_logging();

    // Panic cleanup hook logs backtrace summary (zero PII)
    std::panic::set_hook(Box::new(|info| {
        let loc = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "unknown".to_string());
        log_error("panic", &format!("cat-overlay panicked at {loc}"));
    }));

    // Single-instance session-scoped mutex (`Local\DesktopCat_Overlay_v1`):
    // second launch signals the running overlay to unlock click-through & summon controls!
    let _instance_mutex = unsafe {
        match CreateMutexW(None, false, w!("Local\\DesktopCat_Overlay_v1")) {
            Ok(h) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    if let Ok(existing) =
                        FindWindowW(w!("DesktopCatHelperHost"), PCWSTR::null())
                    {
                        if !existing.0.is_null() {
                            let _ = PostMessageW(existing, WM_APP_SUMMON_ESCAPE, WPARAM(0), LPARAM(0));
                        }
                    }
                    let _ = CloseHandle(h);
                    return;
                }
                h
            }
            Err(e) => {
                log_error("startup", &format!("CreateMutexW failed: {e}"));
                return;
            }
        }
    };

    let hinst: HINSTANCE = unsafe { GetModuleHandleW(PCWSTR::null()).unwrap_or_default().into() };

    // Load config + user profiles + reconcile auto-start (respecting StartupApproved)
    let mut store = ConfigStore::load_or_create();
    reconcile_autostart(store.config.auto_start);

    // Create invisible helper shell (tray, hotkeys, TaskbarCreated, KB135788 context menus)
    let mut shell = match HelperShell::create(hinst, Some(helper_wndproc)) {
        Ok(s) => s,
        Err(e) => {
            log_error("startup", &e);
            return;
        }
    };
    TASKBAR_MSG_ID.with(|m| *m.borrow_mut() = shell.taskbar_created_msg);
    let mut hotkey_conflicts = shell.register_hotkeys(&store.config.hotkeys);

    // Register lock-screen & display-off notifications on helper HWND
    let power_notify = unsafe {
        let _ = WTSRegisterSessionNotification(shell.hwnd, NOTIFY_FOR_THIS_SESSION);
        RegisterPowerSettingNotification(
            HANDLE(shell.hwnd.0),
            &GUID_CONSOLE_DISPLAY_STATE,
            windows::Win32::UI::WindowsAndMessaging::REGISTER_NOTIFICATION_FLAGS(0),
        )
        .ok()
    };

    // Hook foreground / move-size / minimize transitions (NEVER global EVENT_OBJECT_LOCATIONCHANGE)
    let winevent_hooks: Vec<HWINEVENTHOOK> = unsafe {
        [
            EVENT_SYSTEM_FOREGROUND,
            EVENT_SYSTEM_MOVESIZEEND,
            EVENT_SYSTEM_MINIMIZEEND,
        ]
        .into_iter()
        .map(|ev| {
            SetWinEventHook(
                ev,
                ev,
                None,
                Some(win_event_cb),
                0,
                0,
                WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
            )
        })
        .filter(|h| !h.is_invalid())
        .collect()
    };

    // Create transparent always-on-top non-activating layered overlay HWND
    let overlay_hwnd = match create_overlay_window(hinst, store.config.effective_click_through()) {
        Ok(h) => h,
        Err(e) => {
            log_error("startup", &e);
            return;
        }
    };
    OVERLAY_HWND_TLS.with(|h| *h.borrow_mut() = overlay_hwnd);
    CLICK_THROUGH_LOCKED.with(|c| *c.borrow_mut() = store.config.effective_click_through());

    // Initial target monitor lookup (honoring saved monitor_device_name), move-before-DPI so
    // secondary monitors start at their native DPI on frame 1!
    let initial_dpi = unsafe { GetDpiForWindow(overlay_hwnd).max(96) };
    let (init_cw, init_ch) = compute_canvas_px(store.config.scale, initial_dpi);
    let (work_area, active_mon_name) =
        monitor_work_area_for_anchor(&store.config.anchor, init_cw as i32, init_ch as i32);
    let (pre_x, pre_y) =
        anchor_to_work_area_pos(&store.config.anchor, init_cw as i32, init_ch as i32, work_area);
    unsafe {
        let _ = SetWindowPos(
            overlay_hwnd,
            HWND_TOPMOST,
            pre_x,
            pre_y,
            0,
            0,
            SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
        );
    }
    let mut dpi = unsafe { GetDpiForWindow(overlay_hwnd).max(96) };
    let (mut cw, mut ch) = compute_canvas_px(store.config.scale, dpi);
    let (mut win_x, mut win_y) =
        anchor_to_work_area_pos(&store.config.anchor, cw as i32, ch as i32, work_area);
    if store.config.anchor.monitor_device_name.is_empty() && !active_mon_name.is_empty() {
        store.config.anchor.monitor_device_name = active_mon_name;
    }

    let mut renderer = Renderer::new(cw, ch);
    let mut surface = match LayeredSurface::new(cw, ch) {
        Ok(s) => s,
        Err(e) => {
            log_error("startup", &e);
            return;
        }
    };

    // Wake events & high-resolution waitable timer (no timeBeginPeriod(1), no DwmFlush!)
    let (frame_timer, input_wake_ev, config_wake_ev, ipc_wake_ev) = unsafe {
        let timer = CreateWaitableTimerExW(
            None,
            PCWSTR::null(),
            CREATE_WAITABLE_TIMER_HIGH_RESOLUTION,
            TIMER_ALL_ACCESS.0,
        )
        .or_else(|_| CreateWaitableTimerExW(None, PCWSTR::null(), 0, TIMER_ALL_ACCESS.0))
        .expect("waitable timer");
        let in_ev = CreateEventW(None, false, false, PCWSTR::null()).expect("input event");
        let cfg_ev = CreateEventW(None, false, false, PCWSTR::null()).expect("config event");
        let ipc_ev = CreateEventW(None, false, false, PCWSTR::null()).expect("ipc event");
        (timer, in_ev, cfg_ev, ipc_ev)
    };

    let mailbox = Arc::new(InputMailbox::new(input_wake_ev));
    let mut raw_worker = RawInputWorker::spawn(mailbox.clone());
    let config_watcher = ConfigWatcher::spawn(config_wake_ev);

    let mut sim = PetSim::default();
    let mut active_profile = resolve_profile(&store.config.active_profile_id, &store.user_profiles);
    let mut preview_override: Option<CatProfile> = None;
    let mut pipe_session: Option<PipeSession> = None;
    let mut capture_stage: Option<CaptureStageWindow> = if store.config.capture_stage_enabled {
        CaptureStageWindow::open()
    } else {
        None
    };

    // Initial render & show without stealing focus
    let snap0 = sim.snapshot();
    let hb0 = renderer.render(
        &active_profile,
        &snap0,
        RenderOptions {
            backdrop: BackdropMode::Transparent,
            chroma_hard_edge: false,
            show_keycap_legends: store.config.showcase_mode && active_profile.peripherals.show_keycap_legends,
        },
    );
    HEAD_RECT_PX.with(|h| *h.borrow_mut() = hb0.head_rect_px);
    surface.present_rgba(
        overlay_hwnd,
        Some(POINT { x: win_x, y: win_y }),
        true,
        renderer.rgba_bytes(),
        store.config.opacity,
        None,
    );
    unsafe {
        let _ = ShowWindow(overlay_hwnd, SW_SHOWNOACTIVATE);
    }
    log_info("startup", "cat-overlay active");

    let freq = qpc_freq();
    let mut last_step_qpc = qpc_now();
    let mut next_deadline_qpc = last_step_qpc;
    let mut last_fg_check = Instant::now() - Duration::from_secs(5);
    let mut last_power_check = Instant::now() - Duration::from_secs(10);
    let mut last_rate_tick = Instant::now();
    let mut on_battery = is_on_battery();
    let mut eco_qos_enabled = false;
    let mut overlay_visible = true;
    let mut fullscreen_reason = "none";
    let mut uipi_stale_releases: u64 = 0;
    let mut dt_clamped_count: u64 = 0;
    let mut events_per_sec: u32 = 0;
    let mut pipe_nonce: u64 = 1;
    let mut force_full_present = false;
    let mut force_immediate_frame = false;
    let mut was_in_head = false;
    let mut render_avg_us: f64 = 0.0;
    let mut dirty_pct_avg: f64 = 100.0;

    let wait_handles = [frame_timer, input_wake_ev, config_wake_ev, ipc_wake_ev];

    // Main Win32 event + high-res waitable-timer loop
    while !GLOBAL_QUIT.load(Ordering::Acquire) {
        // Drain all queued Win32 messages first
        unsafe {
            let mut msg = MSG::default();
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                if msg.message == windows::Win32::UI::WindowsAndMessaging::WM_QUIT {
                    GLOBAL_QUIT.store(true, Ordering::Release);
                    break;
                }
                let _ = DispatchMessageW(&msg);
            }
        }
        if GLOBAL_QUIT.load(Ordering::Acquire) {
            break;
        }

        // Process deferred UI window callbacks safely outside WndProc re-entrancy
        let actions = UI_ACTIONS.with(|a| std::mem::take(&mut *a.borrow_mut()));
        if let Some((px, py)) = actions.current_pos {
            win_x = px;
            win_y = py;
        }
        if actions.drag_finished {
            let (wa, mon_name) = monitor_work_area_for_hwnd(overlay_hwnd);
            let (cx, cy) = clamp_rect_to_work_area(win_x, win_y, cw as i32, ch as i32, wa);
            if cx != win_x || cy != win_y {
                win_x = cx;
                win_y = cy;
                unsafe {
                    let _ = SetWindowPos(
                        overlay_hwnd,
                        HWND_TOPMOST,
                        win_x,
                        win_y,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                }
            }
            let (nx, ny) = work_area_pos_to_norm(win_x, win_y, cw as i32, ch as i32, wa);
            store.config.anchor.monitor_device_name = mon_name;
            store.config.anchor.norm_x = nx;
            store.config.anchor.norm_y = ny;
            store.config.anchor.fallback_abs_x = win_x;
            store.config.anchor.fallback_abs_y = win_y;
            let _ = store.save_config();
        }

        if let Some((new_dpi, suggested)) = actions.dpi_changed {
            dpi = new_dpi.max(96);
            let (nw, nh) = compute_canvas_px(store.config.scale, dpi);
            cw = nw;
            ch = nh;
            renderer.resize(cw, ch);
            let _ = surface.resize_if_needed(cw, ch);
            let is_dragging = DRAG_STATE.with(|d| {
                let mut d = d.borrow_mut();
                d.win_w = cw as i32;
                d.win_h = ch as i32;
                d.active
            });
            if !is_dragging {
                win_x = suggested.left;
                win_y = suggested.top;
            }
            force_full_present = true;
            force_immediate_frame = true;
            sim.mark_dirty();
        }

        if actions.display_changed {
            let (wa, mon_name) =
                monitor_work_area_for_anchor(&store.config.anchor, cw as i32, ch as i32);
            let (cx, cy) =
                anchor_to_work_area_pos(&store.config.anchor, cw as i32, ch as i32, wa);
            win_x = cx;
            win_y = cy;
            if !mon_name.is_empty() {
                store.config.anchor.monitor_device_name = mon_name;
            }
            unsafe {
                let _ = SetWindowPos(
                    overlay_hwnd,
                    HWND_TOPMOST,
                    win_x,
                    win_y,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            force_full_present = true;
            force_immediate_frame = true;
            sim.mark_dirty();
        }

        if actions.taskbar_recreated {
            shell.add_or_restore_tray("Desktop Cat Companion");
        }
        if let Some(locked) = actions.session_locked {
            sim.set_session_locked(locked);
            if !locked {
                force_full_present = true;
                force_immediate_frame = true;
            }
        }
        if let Some(off) = actions.display_off {
            sim.set_display_off(off);
            if !off {
                force_full_present = true;
                force_immediate_frame = true;
            }
        }
        if actions.pointer_left {
            was_in_head = false;
            sim.on_pointer_leave();
        }
        if let Some((hx, hy)) = actions.hover_point_px {
            eval_head_scratch(
                hx,
                hy,
                dpi,
                any_mouse_button_physically_down(),
                &store.config,
                &mut sim,
                &mut was_in_head,
            );
        }

        let mut trigger_open_designer = actions.open_designer;
        if actions.summon_escape {
            // Single-instance relaunch escape hatch: unlock click-through + clamp to primary screen + open menu
            store.config.click_through_locked = false;
            if store.config.opacity < 0.25 {
                store.config.opacity = 1.0;
            }
            apply_click_through_style(overlay_hwnd, false);
            let (wa, _) = monitor_work_area_for_rect(RectI32 {
                left: 0,
                top: 0,
                right: 100,
                bottom: 100,
            });
            let (cx, cy) = clamp_rect_to_work_area(win_x, win_y, cw as i32, ch as i32, wa);
            win_x = cx;
            win_y = cy;
            let _ = store.save_config();
            force_full_present = true;
            force_immediate_frame = true;
            sim.wake_active();
            trigger_open_designer = true;
        }
        if actions.toggle_lock {
            store.config.click_through_locked = !store.config.click_through_locked;
            apply_click_through_style(overlay_hwnd, store.config.effective_click_through());
            let _ = store.save_config();
            update_tray_status(&shell, &store.config, &sim);
        }
        if actions.toggle_privacy {
            sim.toggle_manual_privacy();
            mailbox.set_privacy_state(
                sim.is_privacy_active(),
                sim.privacy_source() == PrivacySource::Manual,
            );
            force_immediate_frame = true;
            update_tray_status(&shell, &store.config, &sim);
        }
        if actions.open_context_menu {
            let monitors = enumerate_monitors();
            let (_, cur_mon) = monitor_work_area_for_hwnd(overlay_hwnd);
            let cmd = shell.show_context_menu(
                &store.config,
                sim.manual_privacy(),
                &store.user_profiles,
                &monitors,
                &cur_mon,
            );
            // Reset step timestamp after modal menu so spring physics never sees a modal `dt` spike!
            last_step_qpc = qpc_now();
            match cmd {
                CMD_OPEN_DESIGNER => trigger_open_designer = true,
                CMD_TOGGLE_LOCK => {
                    store.config.click_through_locked = !store.config.click_through_locked;
                    apply_click_through_style(overlay_hwnd, store.config.effective_click_through());
                    let _ = store.save_config();
                }
                CMD_TOGGLE_PRIVACY => {
                    sim.toggle_manual_privacy();
                    mailbox.set_privacy_state(
                        sim.is_privacy_active(),
                        sim.privacy_source() == PrivacySource::Manual,
                    );
                    force_immediate_frame = true;
                }
                CMD_TOGGLE_SHOWCASE => {
                    store.config.showcase_mode = !store.config.showcase_mode;
                    let _ = store.save_config();
                }
                CMD_TOGGLE_POPUPS => {
                    store.config.show_floating_popups = !store.config.show_floating_popups;
                    let _ = store.save_config();
                }
                CMD_TOGGLE_CAPTURE_STAGE => {
                    store.config.capture_stage_enabled = !store.config.capture_stage_enabled;
                    if store.config.capture_stage_enabled {
                        if capture_stage.is_none() {
                            capture_stage = CaptureStageWindow::open();
                        }
                    } else {
                        capture_stage = None;
                    }
                    let _ = store.save_config();
                    GLOBAL_FG_DIRTY.store(true, Ordering::Release);
                }
                CMD_TOGGLE_AUTOSTART => {
                    store.config.auto_start = !store.config.auto_start;
                    reconcile_autostart(store.config.auto_start);
                    let _ = store.save_config();
                }
                CMD_RESET_POS => {
                    let (wa, mon) = monitor_work_area_for_rect(RectI32 {
                        left: 0,
                        top: 0,
                        right: 100,
                        bottom: 100,
                    });
                    store.config.anchor.monitor_device_name = mon;
                    store.config.anchor.norm_x = 0.96;
                    store.config.anchor.norm_y = 0.96;
                    let (nx, ny) =
                        anchor_to_work_area_pos(&store.config.anchor, cw as i32, ch as i32, wa);
                    win_x = nx;
                    win_y = ny;
                    let _ = store.save_config();
                    force_full_present = true;
                    force_immediate_frame = true;
                    sim.wake_active();
                }
                CMD_COPY_DIAG => {
                    let diag = build_diagnostics(
                        &sim,
                        &renderer,
                        &store,
                        on_battery,
                        dpi,
                        cw,
                        ch,
                        overlay_visible,
                        fullscreen_reason,
                        mailbox.registered.load(Ordering::Relaxed),
                        events_per_sec,
                        uipi_stale_releases,
                        dt_clamped_count,
                        render_avg_us.round() as u32,
                        dirty_pct_avg.round().clamp(0.0, 100.0) as u8,
                        &hotkey_conflicts,
                        pipe_session.as_ref(),
                    );
                    if let Ok(json) = serde_json::to_string_pretty(&diag) {
                        copy_text_to_clipboard(shell.hwnd, &json);
                    }
                }
                CMD_OPEN_CONFIG_DIR => {
                    let _ = Command::new("explorer.exe").arg(app_data_dir()).spawn();
                }
                CMD_SNAP_BR | CMD_SNAP_BL | CMD_SNAP_TR | CMD_SNAP_TL => {
                    let (nx, ny) = match cmd {
                        CMD_SNAP_BL => (0.04, 0.96),
                        CMD_SNAP_TR => (0.96, 0.04),
                        CMD_SNAP_TL => (0.04, 0.04),
                        _ => (0.96, 0.96),
                    };
                    let (wa, mon) = monitor_work_area_for_hwnd(overlay_hwnd);
                    store.config.anchor.monitor_device_name = mon;
                    store.config.anchor.norm_x = nx;
                    store.config.anchor.norm_y = ny;
                    let (px, py) =
                        anchor_to_work_area_pos(&store.config.anchor, cw as i32, ch as i32, wa);
                    win_x = px;
                    win_y = py;
                    store.config.anchor.fallback_abs_x = win_x;
                    store.config.anchor.fallback_abs_y = win_y;
                    let _ = store.save_config();
                    force_full_present = true;
                    force_immediate_frame = true;
                    sim.wake_active();
                }
                CMD_SCALE_MINI | CMD_SCALE_SMALL | CMD_SCALE_NORMAL | CMD_SCALE_LARGE => {
                    store.config.scale = match cmd {
                        CMD_SCALE_MINI => 0.6,
                        CMD_SCALE_SMALL => 0.85,
                        CMD_SCALE_LARGE => 1.5,
                        _ => 1.0,
                    };
                    let (nw, nh) = compute_canvas_px(store.config.scale, dpi);
                    if nw != cw || nh != ch {
                        cw = nw;
                        ch = nh;
                        renderer.resize(cw, ch);
                        let _ = surface.resize_if_needed(cw, ch);
                    }
                    let (wa, _) = monitor_work_area_for_hwnd(overlay_hwnd);
                    let (px, py) =
                        anchor_to_work_area_pos(&store.config.anchor, cw as i32, ch as i32, wa);
                    win_x = px;
                    win_y = py;
                    store.config.anchor.fallback_abs_x = win_x;
                    store.config.anchor.fallback_abs_y = win_y;
                    let _ = store.save_config();
                    force_full_present = true;
                    force_immediate_frame = true;
                    sim.wake_active();
                }
                c if (CMD_MONITOR_BASE..CMD_MONITOR_BASE + 16).contains(&c) => {
                    let idx = (c - CMD_MONITOR_BASE) as usize;
                    if let Some(m) = monitors.get(idx) {
                        store.config.anchor.monitor_device_name = m.device_name.clone();
                        let (px, py) = anchor_to_work_area_pos(
                            &store.config.anchor,
                            cw as i32,
                            ch as i32,
                            m.work_area,
                        );
                        win_x = px;
                        win_y = py;
                        store.config.anchor.fallback_abs_x = win_x;
                        store.config.anchor.fallback_abs_y = win_y;
                        let _ = store.save_config();
                        force_full_present = true;
                        force_immediate_frame = true;
                        sim.wake_active();
                    }
                }
                CMD_QUIT => {
                    GLOBAL_QUIT.store(true, Ordering::Release);
                    break;
                }
                c if (CMD_PRESET_BASE..CMD_PRESET_BASE + 15).contains(&c) => {
                    let idx = (c - CMD_PRESET_BASE) as usize;
                    if let Some(p) = builtin_presets().get(idx) {
                        store.config.active_profile_id = p.id.clone();
                        active_profile = p.clone();
                        let _ = store.save_config();
                        sim.wake_active();
                    }
                }
                c if (CMD_USER_PROFILE_BASE..CMD_USER_PROFILE_BASE + 200).contains(&c) => {
                    let idx = (c - CMD_USER_PROFILE_BASE) as usize;
                    if let Some(p) = store.user_profiles.get(idx) {
                        store.config.active_profile_id = p.id.clone();
                        active_profile = p.clone();
                        let _ = store.save_config();
                        sim.wake_active();
                    }
                }
                _ => {}
            }
            update_tray_status(&shell, &store.config, &sim);
        }

        // Launch ephemeral `cat-designer.exe` over SID-locked Named Pipe (inherited env var, PID verified)
        if trigger_open_designer {
            if let Some(ref session) = pipe_session {
                // Already running — ask designer to bring itself to foreground
                let _ = session;
                spawn_designer_focus_helper();
            } else {
                pipe_nonce += 1;
                let pid = unsafe { GetCurrentProcessId() };
                let mut sess_id = 0u32;
                unsafe {
                    let _ = ProcessIdToSessionId(pid, &mut sess_id);
                }
                let pipe_name = format!(r"\\.\pipe\DesktopCat_IPC_v1_{sess_id}_{pid}_{pipe_nonce}");
                let expected_pid = Arc::new(Mutex::new(None));
                match PipeSession::start(pipe_name.clone(), expected_pid.clone(), ipc_wake_ev) {
                    Ok(session) => {
                        if let Some(child_pid) = spawn_designer_binary(&pipe_name) {
                            if let Ok(mut ep) = expected_pid.lock() {
                                *ep = Some(child_pid);
                            }
                            pipe_session = Some(session);
                            sim.set_designer_connected(true);
                        } else {
                            log_warn("ipc", "cat-designer.exe not found next to cat-overlay.exe");
                        }
                    }
                    Err(e) => log_warn("ipc", &format!("failed starting pipe server: {e}")),
                }
            }
        }

        // Check Capture Stage user close
        if let Some(ref mut stage) = capture_stage {
            if stage.was_closed_by_user() {
                capture_stage = None;
                store.config.capture_stage_enabled = false;
                let _ = store.save_config();
                GLOBAL_FG_DIRTY.store(true, Ordering::Release);
            }
        }

        // Hot-reload `config.json` (150 ms debounced + hash-deduplicated)
        if config_watcher.poll_debounced_ready() && store.reload_config_with_recovery() {
            store.reload_user_profiles();
            active_profile =
                resolve_profile(&store.config.active_profile_id, &store.user_profiles);
            apply_click_through_style(overlay_hwnd, store.config.effective_click_through());
            hotkey_conflicts = shell.register_hotkeys(&store.config.hotkeys);
            let (nw, nh) = compute_canvas_px(store.config.scale, dpi);
            if nw != cw || nh != ch {
                cw = nw;
                ch = nh;
                renderer.resize(cw, ch);
                let _ = surface.resize_if_needed(cw, ch);
                let (wa, _) = monitor_work_area_for_hwnd(overlay_hwnd);
                let (px, py) =
                    anchor_to_work_area_pos(&store.config.anchor, cw as i32, ch as i32, wa);
                win_x = px;
                win_y = py;
                store.config.anchor.fallback_abs_x = win_x;
                store.config.anchor.fallback_abs_y = win_y;
            }
            reconcile_autostart(store.config.auto_start);
            force_full_present = true;
            force_immediate_frame = true;
            sim.wake_active();
        }

        // Drain Named Pipe IPC messages from `cat-designer.exe`
        let mut disconnect_pipe = false;
        if let Some(ref session) = pipe_session {
            for ev in session.drain_events() {
                match ev {
                    PipeEvent::Disconnected => {
                        disconnect_pipe = true;
                    }
                    PipeEvent::Message(msg) => match msg {
                        DesignerToOverlay::Hello { .. } => {
                            let diag = build_diagnostics(
                                &sim,
                                &renderer,
                                &store,
                                on_battery,
                                dpi,
                                cw,
                                ch,
                                overlay_visible,
                                fullscreen_reason,
                                mailbox.registered.load(Ordering::Relaxed),
                                events_per_sec,
                                uipi_stale_releases,
                                dt_clamped_count,
                                render_avg_us.round() as u32,
                                dirty_pct_avg.round().clamp(0.0, 100.0) as u8,
                                &hotkey_conflicts,
                                Some(session),
                            );
                            session.send(&OverlayToDesigner::HelloAck {
                                protocol_version: PROTOCOL_VERSION,
                                overlay_pid: unsafe { GetCurrentProcessId() },
                                config: store.config.clone(),
                                user_profiles: store.user_profiles.clone(),
                                diagnostics: diag,
                            });
                        }
                        DesignerToOverlay::PreviewPatch {
                            profile_draft,
                            pose_override,
                            ..
                        } => {
                            preview_override = Some(profile_draft);
                            sim.set_pose_override(pose_override);
                            force_full_present = true;
                            force_immediate_frame = true;
                            sim.wake_active();
                        }
                        DesignerToOverlay::CancelPreview => {
                            preview_override = None;
                            sim.set_pose_override(PoseOverride::Auto);
                            force_full_present = true;
                            force_immediate_frame = true;
                            sim.wake_active();
                        }
                        DesignerToOverlay::CommitProfile {
                            profile,
                            make_active,
                        } => {
                            match store.save_user_profile(profile) {
                                Ok(saved) => {
                                    if make_active {
                                        store.config.active_profile_id = saved.id.clone();
                                        active_profile = saved;
                                        let _ = store.save_config();
                                    }
                                    preview_override = None;
                                    session.send(&OverlayToDesigner::CommitAck {
                                        ok: true,
                                        error: None,
                                    });
                                    sim.wake_active();
                                }
                                Err(e) => {
                                    session.send(&OverlayToDesigner::CommitAck {
                                        ok: false,
                                        error: Some(e),
                                    });
                                }
                            }
                        }
                        DesignerToOverlay::DeleteProfile { profile_id } => {
                            let res = store.delete_user_profile(&profile_id);
                            active_profile = resolve_profile(
                                &store.config.active_profile_id,
                                &store.user_profiles,
                            );
                            session.send(&OverlayToDesigner::CommitAck {
                                ok: res.is_ok(),
                                error: res.err(),
                            });
                            sim.wake_active();
                        }
                        DesignerToOverlay::UpdateConfig { mut config } => {
                            config.sanitize();
                            let prev_hotkeys = store.config.hotkeys.clone();
                            let prev_auto = store.config.auto_start;
                            store.config = config;
                            active_profile = resolve_profile(
                                &store.config.active_profile_id,
                                &store.user_profiles,
                            );
                            apply_click_through_style(
                                overlay_hwnd,
                                store.config.effective_click_through(),
                            );
                            if store.config.hotkeys != prev_hotkeys {
                                hotkey_conflicts = shell.register_hotkeys(&store.config.hotkeys);
                            }
                            if store.config.auto_start != prev_auto {
                                reconcile_autostart(store.config.auto_start);
                            }
                            if store.config.capture_stage_enabled && capture_stage.is_none() {
                                capture_stage = CaptureStageWindow::open();
                            } else if !store.config.capture_stage_enabled {
                                capture_stage = None;
                            }
                            let (nw, nh) = compute_canvas_px(store.config.scale, dpi);
                            if nw != cw || nh != ch {
                                cw = nw;
                                ch = nh;
                                renderer.resize(cw, ch);
                                let _ = surface.resize_if_needed(cw, ch);
                                let (wa, _) = monitor_work_area_for_hwnd(overlay_hwnd);
                                let (px, py) = anchor_to_work_area_pos(
                                    &store.config.anchor,
                                    cw as i32,
                                    ch as i32,
                                    wa,
                                );
                                win_x = px;
                                win_y = py;
                                store.config.anchor.fallback_abs_x = win_x;
                                store.config.anchor.fallback_abs_y = win_y;
                            }
                            let _ = store.save_config();
                            GLOBAL_FG_DIRTY.store(true, Ordering::Release);
                            force_full_present = true;
                            force_immediate_frame = true;
                            sim.wake_active();
                        }
                        DesignerToOverlay::RequestDiagnostics => {
                            let diag = build_diagnostics(
                                &sim,
                                &renderer,
                                &store,
                                on_battery,
                                dpi,
                                cw,
                                ch,
                                overlay_visible,
                                fullscreen_reason,
                                mailbox.registered.load(Ordering::Relaxed),
                                events_per_sec,
                                uipi_stale_releases,
                                dt_clamped_count,
                                render_avg_us.round() as u32,
                                dirty_pct_avg.round().clamp(0.0, 100.0) as u8,
                                &hotkey_conflicts,
                                Some(session),
                            );
                            session.send(&OverlayToDesigner::Diagnostics(diag));
                        }
                        DesignerToOverlay::Ping => {
                            session.send(&OverlayToDesigner::Pong);
                        }
                        DesignerToOverlay::FocusDesigner => {}
                    },
                }
            }
        }
        if disconnect_pipe {
            pipe_session = None;
            preview_override = None; // Revert ephemeral preview cleanly within 1 frame!
            sim.set_designer_connected(false);
            sim.wake_active();
        }

        // Foreground evaluation (event-driven + 1.5 s lazy check while awake)
        let fg_due = GLOBAL_FG_DIRTY.swap(false, Ordering::AcqRel)
            || (!sim.is_parked() && last_fg_check.elapsed() >= Duration::from_millis(1500));
        if fg_due {
            last_fg_check = Instant::now();
            let eval = evaluate_foreground(overlay_hwnd, capture_stage.is_some(), &store.config);
            fullscreen_reason = eval.fullscreen_reason;
            sim.set_auto_privacy(eval.auto_privacy);
            mailbox.set_privacy_state(
                sim.is_privacy_active(),
                sim.privacy_source() == PrivacySource::Manual,
            );

            if eval.foreground_elevated_or_inaccessible && sim.has_any_key_held() {
                sim.release_all_keys();
                uipi_stale_releases += 1;
            }

            if eval.should_hide_for_game && overlay_visible {
                unsafe {
                    let _ = ShowWindow(overlay_hwnd, SW_HIDE);
                }
                overlay_visible = false;
                raw_worker.set_enabled(false); // Thread-affined RIDEV_REMOVE!
                sim.set_game_hidden(true);
            } else if !eval.should_hide_for_game && !overlay_visible {
                overlay_visible = true;
                raw_worker.set_enabled(true);
                sim.set_game_hidden(false);
                force_full_present = true;
                force_immediate_frame = true;
                unsafe {
                    let _ = ShowWindow(overlay_hwnd, SW_SHOWNOACTIVATE);
                }
            }
        }

        // Battery check every 10 s + events/sec rate counter
        if last_power_check.elapsed() >= Duration::from_secs(10) {
            last_power_check = Instant::now();
            on_battery = is_on_battery();
        }
        if last_rate_tick.elapsed() >= Duration::from_secs(1) {
            last_rate_tick = Instant::now();
            events_per_sec = mailbox.events_counter.swap(0, Ordering::Relaxed);
        }

        // Drain coalesced Raw Input (only when visible or Capture Stage active)
        if overlay_visible || capture_stage.is_some() {
            let drained = mailbox.drain();
            if drained.privacy_activity {
                sim.on_privacy_activity();
            }
            for (k, down) in drained.keys {
                sim.on_key_event(k, down, &store.config);
            }
            if drained.mouse_dx != 0
                || drained.mouse_dy != 0
                || drained.wheel_delta != 0
                || drained.left_btn.is_some()
                || drained.left_clicked
                || drained.right_btn.is_some()
                || drained.right_clicked
            {
                let gaze = compute_cursor_gaze(overlay_hwnd);
                sim.on_mouse_input(
                    drained.mouse_dx,
                    drained.mouse_dy,
                    drained.left_btn,
                    drained.left_clicked,
                    drained.right_btn,
                    drained.right_clicked,
                    drained.wheel_delta,
                    gaze,
                );
                // Event-driven localized head scratching when click-through lock is enabled
                // (reuses Raw Input mouse-move wakeups; zero timers and 0 wakeups/sec in Parked0!)
                if store.config.effective_click_through()
                    && (drained.mouse_dx != 0 || drained.mouse_dy != 0)
                {
                    if let Some((lx, ly)) = cursor_in_overlay_px(overlay_hwnd) {
                        eval_head_scratch(
                            lx,
                            ly,
                            dpi,
                            any_mouse_button_physically_down(),
                            &store.config,
                            &mut sim,
                            &mut was_in_head,
                        );
                    }
                }
            }
            // Stale-key UIPI safety fallback for keys held > 2.5 s when OS reports key no longer pressed
            if sim.has_any_key_held() && !any_common_key_physically_down() {
                let rel = sim.release_stale_keys(2.5);
                uipi_stale_releases += rel as u64;
            }
        }

        // Step simulation & render on QPC deadline (Bug #6 fix: never let passive hover bypass pacing!)
        let now_qpc = qpc_now();
        let stage = sim.pacing();
        let period_ms = stage.period_ms(on_battery);
        let should_step = if period_ms.is_some() {
            now_qpc >= next_deadline_qpc || force_immediate_frame
        } else {
            sim.is_dirty() || force_immediate_frame
        };

        if should_step && (overlay_visible || capture_stage.is_some()) {
            force_immediate_frame = false;
            let raw_dt = ((now_qpc - last_step_qpc).max(0) as f64 / freq) as f32;
            if raw_dt > MAX_SIM_DT {
                dt_clamped_count += 1;
            }
            last_step_qpc = now_qpc;
            sim.step(raw_dt);

            let display_profile = preview_override.as_ref().unwrap_or(&active_profile);
            let snap = sim.snapshot();

            if overlay_visible {
                let t0 = qpc_now();
                let hb = renderer.render(
                    display_profile,
                    &snap,
                    RenderOptions {
                        backdrop: BackdropMode::Transparent,
                        chroma_hard_edge: false,
                        show_keycap_legends: store.config.showcase_mode
                            && display_profile.peripherals.show_keycap_legends,
                    },
                );
                let render_us = ((qpc_now() - t0).max(0) as f64 / freq) * 1_000_000.0;
                render_avg_us = if render_avg_us <= 0.1 {
                    render_us
                } else {
                    render_avg_us * 0.90 + render_us * 0.10
                };

                HEAD_RECT_PX.with(|h| *h.borrow_mut() = hb.head_rect_px);

                // Bug #1 fix: RECT.right = x + w, RECT.bottom = y + h (NOT w and h!)
                let left = hb.dirty_rect_px.x.floor().clamp(0.0, cw as f32) as i32;
                let top = hb.dirty_rect_px.y.floor().clamp(0.0, ch as f32) as i32;
                let right = (hb.dirty_rect_px.x + hb.dirty_rect_px.w)
                    .ceil()
                    .clamp(left as f32, cw as f32) as i32;
                let bottom = (hb.dirty_rect_px.y + hb.dirty_rect_px.h)
                    .ceil()
                    .clamp(top as f32, ch as f32) as i32;
                let dirty_r = RECT {
                    left,
                    top,
                    right,
                    bottom,
                };
                let dirty_area = ((right - left).max(0) as f64) * ((bottom - top).max(0) as f64);
                let total_area = (cw as f64 * ch as f64).max(1.0);
                let pct = (dirty_area / total_area) * 100.0;
                dirty_pct_avg = dirty_pct_avg * 0.90 + pct * 0.10;

                let is_full_dirty = force_full_present
                    || (left == 0 && top == 0 && right == cw as i32 && bottom == ch as i32);
                let ok = if is_full_dirty {
                    surface.present_rgba(
                        overlay_hwnd,
                        Some(POINT { x: win_x, y: win_y }),
                        true,
                        renderer.rgba_bytes(),
                        store.config.opacity,
                        None,
                    )
                } else {
                    surface.present_rgba(
                        overlay_hwnd,
                        None,
                        false,
                        renderer.rgba_bytes(),
                        store.config.opacity,
                        Some(dirty_r),
                    )
                };
                force_full_present = !ok;
            }

            if let Some(ref mut stage_win) = capture_stage {
                stage_win.render_and_blit(
                    display_profile,
                    &snap,
                    store.config.chroma_key_color,
                    store.config.chroma_hard_edge,
                    store.config.showcase_mode && display_profile.peripherals.show_keycap_legends,
                );
            }
            sim.clear_dirty();

            // Advance deadline cleanly without catch-up spiral
            if let Some(p_ms) = sim.pacing().period_ms(on_battery) {
                let period_qpc = ((p_ms / 1000.0) * freq) as i64;
                next_deadline_qpc += period_qpc;
                if now_qpc > next_deadline_qpc + 2 * period_qpc {
                    next_deadline_qpc = now_qpc + period_qpc;
                }
            }
        }

        // Toggle EcoQoS (enabled in Idle/Park, disabled in Active60)
        let want_eco = matches!(sim.pacing(), PacingStage::Drowsy12 | PacingStage::Parked0);
        if want_eco != eco_qos_enabled {
            set_eco_qos(want_eco);
            eco_qos_enabled = want_eco;
        }

        // Arm high-resolution waitable timer OR park cleanly at 0 wakeups/sec!
        let next_stage = sim.pacing();
        if let Some(p_ms) = next_stage.period_ms(on_battery) {
            mailbox.overlay_parked.store(false, Ordering::SeqCst);
            let now = qpc_now();
            let remain_sec = ((next_deadline_qpc - now).max(0) as f64 / freq).min(p_ms / 1000.0);
            let rel_100ns = -((remain_sec * 10_000_000.0).round() as i64).max(10_000);
            unsafe {
                let _ = SetWaitableTimer(frame_timer, &rel_100ns, 0, None, None, false);
                let _ = MsgWaitForMultipleObjectsEx(
                    Some(&wait_handles),
                    1000,
                    QS_ALLINPUT,
                    MWMO_INPUTAVAILABLE,
                );
            }
        } else {
            // Strict SeqCst lost-wakeup-free Park handshake: reset the wake event BEFORE
            // publishing `overlay_parked = true` so an input arriving after the store can never
            // have its `SetEvent` cleared by `ResetEvent`.
            unsafe {
                let _ = CancelWaitableTimer(frame_timer);
                let _ = ResetEvent(input_wake_ev);
            }
            let seq_before = mailbox.input_seq.load(Ordering::SeqCst);
            mailbox.overlay_parked.store(true, Ordering::SeqCst);
            let seq_after = mailbox.input_seq.load(Ordering::SeqCst);
            if seq_after != seq_before {
                mailbox.overlay_parked.store(false, Ordering::SeqCst);
                continue;
            }
            unsafe {
                let _ = MsgWaitForMultipleObjectsEx(
                    Some(&wait_handles[1..]), // Exclude disarmed timer; 0 wakeups/sec!
                    windows::Win32::System::Threading::INFINITE,
                    QS_ALLINPUT,
                    MWMO_INPUTAVAILABLE,
                );
            }
            mailbox.overlay_parked.store(false, Ordering::SeqCst);
            last_step_qpc = qpc_now();
            next_deadline_qpc = last_step_qpc;
        }
    }

    // Clean shutdown
    log_info("shutdown", "cat-overlay exiting cleanly");
    if let Some(mut s) = pipe_session {
        s.shutdown();
    }
    raw_worker.shutdown();
    drop(capture_stage);
    unsafe {
        for h in winevent_hooks {
            let _ = UnhookWinEvent(h);
        }
        if let Some(pn) = power_notify {
            let _ = UnregisterPowerSettingNotification(pn);
        }
        let _ = WTSUnRegisterSessionNotification(shell.hwnd);
        let _ = DestroyWindow(overlay_hwnd);
        let _ = CloseHandle(frame_timer);
        let _ = CloseHandle(input_wake_ev);
        let _ = CloseHandle(config_wake_ev);
        let _ = CloseHandle(ipc_wake_ev);
    }
    drop(shell);
}

fn create_overlay_window(hinst: HINSTANCE, click_through: bool) -> Result<HWND, String> {
    unsafe {
        let class_name = w!("DesktopCatOverlay25D");
        let wc = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(overlay_wndproc),
            hInstance: hinst,
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            lpszClassName: class_name,
            ..Default::default()
        };
        let _ = RegisterClassExW(&wc);

        let mut ex = WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW;
        if click_through {
            ex |= WS_EX_TRANSPARENT;
        }
        CreateWindowExW(
            ex,
            class_name,
            w!("Desktop Cat Companion"),
            WS_POPUP,
            0,
            0,
            360,
            300,
            None,
            None,
            hinst,
            None,
        )
        .map_err(|e| format!("create overlay hwnd: {e}"))
    }
}

fn compute_cursor_gaze(overlay_hwnd: HWND) -> Option<Vec2> {
    unsafe {
        let mut pt = POINT::default();
        if GetCursorPos(&mut pt).is_err() {
            return None;
        }
        let mut rc = RECT::default();
        if GetWindowRect(overlay_hwnd, &mut rc).is_err() {
            return None;
        }
        let cx = (rc.left + rc.right) as f32 * 0.5;
        let cy = (rc.top + rc.bottom) as f32 * 0.36; // head center
        let nx = (pt.x as f32 - cx) / 420.0;
        let ny = (pt.y as f32 - cy) / 340.0;
        // Radial tanh preserves true gaze angle on distant diagonal multi-monitor cursors
        let r = (nx * nx + ny * ny).sqrt();
        if r < 1e-4 {
            Some(Vec2::default())
        } else {
            let scale = r.tanh() / r;
            Some(Vec2::new(nx * scale, ny * scale))
        }
    }
}

fn cursor_in_overlay_px(overlay_hwnd: HWND) -> Option<(f32, f32)> {
    unsafe {
        let mut pt = POINT::default();
        if GetCursorPos(&mut pt).is_err() {
            return None;
        }
        let mut rc = RECT::default();
        if GetWindowRect(overlay_hwnd, &mut rc).is_err() {
            return None;
        }
        Some(((pt.x - rc.left) as f32, (pt.y - rc.top) as f32))
    }
}

fn any_mouse_button_physically_down() -> bool {
    unsafe {
        // VK_LBUTTON (0x01), VK_RBUTTON (0x02), VK_MBUTTON (0x04)
        for vk in [0x01, 0x02, 0x04] {
            if (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 {
                return true;
            }
        }
    }
    false
}

fn eval_head_scratch(
    hx: f32,
    hy: f32,
    dpi: u32,
    buttons_held: bool,
    cfg: &pet_core::Config,
    sim: &mut PetSim,
    was_in_head: &mut bool,
) {
    let (inside, norm_u, norm_v) = HEAD_RECT_PX.with(|r| {
        let rect = *r.borrow();
        let rx = (rect.w * 0.5).max(1.0);
        let ry = (rect.h * 0.5).max(1.0);
        let cx = rect.x + rx;
        let cy = rect.y + ry;
        let pad = if *was_in_head { 10.0 } else { 0.0 };
        let u = (hx - cx) / (rx + pad);
        let v = (hy - cy) / (ry + pad);
        // Elliptical head region avoids empty-air corner triggers above the ears
        let inside = (u * u + v * v) <= 1.0;
        let nu = ((hx - cx) / rx).clamp(-1.0, 1.0);
        let nv = ((hy - cy) / ry).clamp(-1.0, 1.0);
        (inside, nu, nv)
    });
    *was_in_head = inside;
    let scale = 96.0 / dpi.max(96) as f32;
    let pos_dip = Vec2::new(hx * scale, hy * scale);
    let _ = sim.on_head_scratch(inside, norm_u, norm_v, pos_dip, buttons_held, cfg);
}

fn any_common_key_physically_down() -> bool {
    unsafe {
        for vk in 0x08..=0xFE {
            if (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 {
                return true;
            }
        }
    }
    false
}

fn spawn_designer_binary(pipe_name: &str) -> Option<u32> {
    let exe = std::env::current_exe().ok()?;
    let designer_exe = exe.with_file_name("cat-designer.exe");
    if !designer_exe.exists() {
        return None;
    }
    // Pass pipe name via inherited environment variable (NEVER CLI args!)
    let child = Command::new(designer_exe)
        .env("DESKTOP_CAT_IPC_PIPE", pipe_name)
        .spawn()
        .ok()?;
    Some(child.id())
}

fn spawn_designer_focus_helper() {
    if let Ok(exe) = std::env::current_exe() {
        let designer_exe = exe.with_file_name("cat-designer.exe");
        if designer_exe.exists() {
            let _ = Command::new(designer_exe).spawn();
        }
    }
}

fn update_tray_status(shell: &HelperShell, cfg: &pet_core::Config, sim: &PetSim) {
    let lock = if cfg.effective_click_through() {
        "Locked"
    } else {
        "Unlocked"
    };
    let priv_s = if sim.is_privacy_active() {
        " | Privacy ON"
    } else {
        ""
    };
    shell.update_tooltip(&format!("Desktop Cat ({lock}{priv_s})"));
}

#[allow(clippy::too_many_arguments)]
fn build_diagnostics(
    sim: &PetSim,
    renderer: &Renderer,
    store: &ConfigStore,
    on_battery: bool,
    dpi: u32,
    cw: u32,
    ch: u32,
    overlay_visible: bool,
    fullscreen_reason: &str,
    raw_input_registered: bool,
    raw_input_events_sec: u32,
    uipi_stale_releases: u64,
    dt_clamped_count: u64,
    render_avg_us: u32,
    dirty_rect_avg_pct: u8,
    hotkey_conflicts: &[String],
    pipe: Option<&PipeSession>,
) -> DiagnosticsSnapshot {
    let (gdi, ws_kb) = current_process_stats();
    let mut diag = DiagnosticsSnapshot {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        target_fps: sim.pacing().target_fps(on_battery),
        on_battery,
        dpi,
        canvas_w_px: cw,
        canvas_h_px: ch,
        overlay_visible,
        click_through_effective: store.config.effective_click_through(),
        fullscreen_suppressed: !overlay_visible,
        fullscreen_reason: fullscreen_reason.to_string(),
        raw_input_registered,
        raw_input_events_sec,
        uipi_stale_releases,
        dt_clamped_count,
        gdi_objects: gdi,
        working_set_kb: ws_kb,
        render_avg_us,
        dirty_rect_avg_pct,
        sprite_cache_rebuilds: renderer.cache_rebuilds(),
        spring_nan_resets: sim.spring_nan_resets(),
        hotkey_conflicts: hotkey_conflicts.to_vec(),
        last_config_error: store.last_error.clone(),
        last_ipc_error: pipe.and_then(|p| p.last_error.lock().ok().and_then(|e| e.clone())),
        ..DiagnosticsSnapshot::default()
    };
    diag.set_state_enums(sim.pacing(), sim.privacy_source());
    diag
}

unsafe extern "system" fn win_event_cb(
    _hook: HWINEVENTHOOK,
    _event: u32,
    _hwnd: HWND,
    _id_object: i32,
    _id_child: i32,
    _thread: u32,
    _time: u32,
) {
    GLOBAL_FG_DIRTY.store(true, Ordering::Release);
}

unsafe extern "system" fn helper_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let tb_msg = TASKBAR_MSG_ID.with(|m| *m.borrow());
    if tb_msg != 0 && msg == tb_msg {
        UI_ACTIONS.with(|a| a.borrow_mut().taskbar_recreated = true);
        return LRESULT(0);
    }

    match msg {
        WM_APP_TRAY => {
            let ev = (lparam.0 & 0xFFFF) as u32;
            if ev == WM_RBUTTONUP || ev == WM_LBUTTONUP {
                UI_ACTIONS.with(|a| a.borrow_mut().open_context_menu = true);
            }
            LRESULT(0)
        }
        WM_APP_SUMMON_ESCAPE => {
            UI_ACTIONS.with(|a| a.borrow_mut().summon_escape = true);
            LRESULT(0)
        }
        WM_HOTKEY => {
            match wparam.0 as i32 {
                HOTKEY_ID_CLICK_THROUGH => UI_ACTIONS.with(|a| a.borrow_mut().toggle_lock = true),
                HOTKEY_ID_PRIVACY => UI_ACTIONS.with(|a| a.borrow_mut().toggle_privacy = true),
                HOTKEY_ID_DESIGNER => UI_ACTIONS.with(|a| a.borrow_mut().open_designer = true),
                _ => {}
            }
            LRESULT(0)
        }
        WM_WTSSESSION_CHANGE => {
            let state = wparam.0 as u32;
            if state == WTS_SESSION_LOCK {
                UI_ACTIONS.with(|a| a.borrow_mut().session_locked = Some(true));
            } else if state == WTS_SESSION_UNLOCK {
                UI_ACTIONS.with(|a| a.borrow_mut().session_locked = Some(false));
            }
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            let ev = wparam.0 as u32;
            if ev == PBT_APMSUSPEND {
                UI_ACTIONS.with(|a| a.borrow_mut().display_off = Some(true));
            } else if ev == PBT_APMRESUMEAUTOMATIC {
                UI_ACTIONS.with(|a| a.borrow_mut().display_off = Some(false));
            } else if ev == PBT_POWERSETTINGCHANGE && lparam.0 != 0 {
                let ps = &*(lparam.0 as *const POWERBROADCAST_SETTING);
                if ps.PowerSetting == GUID_CONSOLE_DISPLAY_STATE && ps.DataLength >= 4 {
                    let val = *(ps.Data.as_ptr() as *const u32);
                    UI_ACTIONS.with(|a| a.borrow_mut().display_off = Some(val == 0));
                }
            }
            LRESULT(1)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

unsafe extern "system" fn overlay_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_LBUTTONDOWN => {
            if CLICK_THROUGH_LOCKED.with(|c| *c.borrow()) {
                return LRESULT(0);
            }
            let mut rc = RECT::default();
            let mut pt = POINT::default();
            if GetWindowRect(hwnd, &mut rc).is_ok() && GetCursorPos(&mut pt).is_ok() {
                let w = (rc.right - rc.left).max(1);
                let h = (rc.bottom - rc.top).max(1);
                let gu = ((pt.x - rc.left) as f32 / w as f32).clamp(0.0, 1.0);
                let gv = ((pt.y - rc.top) as f32 / h as f32).clamp(0.0, 1.0);
                DRAG_STATE.with(|d| {
                    let mut d = d.borrow_mut();
                    d.active = true;
                    d.grab_u = gu;
                    d.grab_v = gv;
                    d.win_w = w;
                    d.win_h = h;
                });
                let _ = SetCapture(hwnd);
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let dragging = DRAG_STATE.with(|d| d.borrow().active);
            if dragging {
                let mut pt = POINT::default();
                if GetCursorPos(&mut pt).is_ok() {
                    let (nx, ny) = DRAG_STATE.with(|d| {
                        let d = d.borrow();
                        let nx = pt.x - (d.grab_u * d.win_w as f32).round() as i32;
                        let ny = pt.y - (d.grab_v * d.win_h as f32).round() as i32;
                        (nx, ny)
                    });
                    // Position-only move via SetWindowPos -> zero bitmap re-upload at 1000 Hz!
                    let _ = SetWindowPos(
                        hwnd,
                        HWND_TOPMOST,
                        nx,
                        ny,
                        0,
                        0,
                        SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                    );
                    UI_ACTIONS.with(|a| a.borrow_mut().current_pos = Some((nx, ny)));
                }
            } else if !CLICK_THROUGH_LOCKED.with(|c| *c.borrow()) {
                // Re-arm TME_LEAVE when hover enters
                let armed = TRACKING_LEAVE.with(|t| {
                    let mut t = t.borrow_mut();
                    let was = *t;
                    *t = true;
                    was
                });
                if !armed {
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    let _ = TrackMouseEvent(&mut tme);
                }
                let lx = (lparam.0 & 0xFFFF) as i16 as f32;
                let ly = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
                let in_head = HEAD_RECT_PX.with(|r| r.borrow().contains(lx, ly));
                let cursor_id = if in_head { IDC_HAND } else { IDC_ARROW };
                if let Ok(cur) = LoadCursorW(None, cursor_id) {
                    let _ = SetCursor(cur);
                }
                UI_ACTIONS.with(|a| a.borrow_mut().hover_point_px = Some((lx, ly)));
            }
            LRESULT(0)
        }
        WM_LBUTTONUP | WM_CAPTURECHANGED | WM_CANCELMODE => {
            let was_dragging = DRAG_STATE.with(|d| {
                let mut d = d.borrow_mut();
                let was = d.active;
                d.active = false;
                was
            });
            if was_dragging {
                let _ = ReleaseCapture();
                UI_ACTIONS.with(|a| a.borrow_mut().drag_finished = true);
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            TRACKING_LEAVE.with(|t| *t.borrow_mut() = false);
            UI_ACTIONS.with(|a| a.borrow_mut().pointer_left = true);
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            if !CLICK_THROUGH_LOCKED.with(|c| *c.borrow()) {
                UI_ACTIONS.with(|a| a.borrow_mut().open_context_menu = true);
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let new_dpi = (wparam.0 & 0xFFFF) as u32;
            let suggested = if lparam.0 != 0 {
                *(lparam.0 as *const RECT)
            } else {
                RECT::default()
            };
            UI_ACTIONS.with(|a| a.borrow_mut().dpi_changed = Some((new_dpi, suggested)));
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            UI_ACTIONS.with(|a| a.borrow_mut().display_changed = true);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
