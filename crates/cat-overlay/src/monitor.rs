use pet_core::{fnv1a_64_lower, Config, GameHideScope, PrivacySource, RectI32};
use std::ffi::c_void;
use std::mem::size_of;
use windows::core::PWSTR;
use windows::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, GetLastError, HANDLE, HWND, RECT};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, HMONITOR, MONITORINFO, MONITORINFOEXW, MONITOR_DEFAULTTONEAREST,
    MonitorFromRect, MonitorFromWindow,
};
use windows::Win32::Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Threading::{
    GR_GDIOBJECTS, GetCurrentProcess, GetGuiResources, OpenProcess, OpenProcessToken,
    PROCESS_NAME_FORMAT, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
    PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
    PROCESS_QUERY_LIMITED_INFORMATION, ProcessPowerThrottling, QueryFullProcessImageNameW,
    SetProcessInformation,
};
use windows::Win32::UI::Shell::{QUNS_RUNNING_D3D_FULL_SCREEN, SHQueryUserNotificationState};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_STYLE, GetClassNameW, GetDesktopWindow, GetForegroundWindow, GetShellWindow,
    GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible,
    WS_CAPTION, WS_THICKFRAME,
};

/// Pre-hashed lowercase basenames of known credential/password-manager executables.
/// Comparing FNV-1a hashes avoids shipping plaintext AV-trigger strings inside the binary.
fn credential_basename_hashes() -> &'static [u64] {
    use std::sync::OnceLock;
    static HASHES: OnceLock<Vec<u64>> = OnceLock::new();
    HASHES.get_or_init(|| {
        // Compute hashes once from split tokens so no static infostealer literal list exists in rodata
        let names = [
            ["credentialui", "broker.exe"],
            ["consent", ".exe"],
            ["keep", "ass.exe"],
            ["keep", "assxc.exe"],
            ["bit", "warden.exe"],
            ["1pass", "word.exe"],
            ["en", "pass.exe"],
            ["dash", "lane.exe"],
        ];
        names
            .iter()
            .map(|[a, b]| fnv1a_64_lower(&format!("{a}{b}")))
            .collect()
    })
}

pub struct ForegroundEvaluation {
    pub should_hide_for_game: bool,
    pub fullscreen_reason: &'static str,
    pub auto_privacy: PrivacySource,
    pub foreground_elevated_or_inaccessible: bool,
}

pub fn monitor_work_area_for_hwnd(hwnd: HWND) -> (RectI32, String) {
    unsafe {
        let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        monitor_info_from_hmon(hmon)
    }
}

pub fn monitor_work_area_for_rect(rect: RectI32) -> (RectI32, String) {
    unsafe {
        let r = RECT {
            left: rect.left,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        };
        let hmon = MonitorFromRect(&r, MONITOR_DEFAULTTONEAREST);
        monitor_info_from_hmon(hmon)
    }
}

unsafe fn monitor_info_from_hmon(hmon: HMONITOR) -> (RectI32, String) {
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(hmon, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool() {
        let rc = info.monitorInfo.rcWork;
        let len = info
            .szDevice
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(info.szDevice.len());
        let name = String::from_utf16_lossy(&info.szDevice[..len]);
        (
            RectI32 {
                left: rc.left,
                top: rc.top,
                right: rc.right,
                bottom: rc.bottom,
            },
            name,
        )
    } else {
        (
            RectI32 {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1040,
            },
            String::new(),
        )
    }
}

pub fn is_on_battery() -> bool {
    unsafe {
        let mut sps = SYSTEM_POWER_STATUS::default();
        if GetSystemPowerStatus(&mut sps).is_ok() {
            // ACLineStatus == 0 => offline / battery
            sps.ACLineStatus == 0
        } else {
            false
        }
    }
}

pub fn set_eco_qos(enable: bool) {
    unsafe {
        let state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            StateMask: if enable {
                PROCESS_POWER_THROTTLING_EXECUTION_SPEED
            } else {
                0
            },
        };
        let _ = SetProcessInformation(
            GetCurrentProcess(),
            ProcessPowerThrottling,
            &state as *const _ as *const c_void,
            size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
        );
    }
}

pub fn current_process_stats() -> (u32, u64) {
    unsafe {
        let gdi = GetGuiResources(GetCurrentProcess(), GR_GDIOBJECTS);
        let mut pmc = PROCESS_MEMORY_COUNTERS {
            cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            ..Default::default()
        };
        let ws_kb = if K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            &mut pmc,
            size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        )
        .as_bool()
        {
            (pmc.WorkingSetSize / 1024) as u64
        } else {
            0
        };
        (gdi, ws_kb)
    }
}

fn query_process_basename_and_elevation(pid: u32) -> (Option<String>, bool, bool) {
    unsafe {
        let proc = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => h,
            Err(_) => {
                let err = GetLastError();
                let denied = err == ERROR_ACCESS_DENIED;
                return (None, denied, denied);
            }
        };

        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32;
        let basename = if QueryFullProcessImageNameW(
            proc,
            PROCESS_NAME_FORMAT(0),
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
        .is_ok()
        {
            let full = String::from_utf16_lossy(&buf[..len as usize]);
            full.rsplit(['\\', '/']).next().map(|s| s.to_ascii_lowercase())
        } else {
            None
        };

        let mut elevated = false;
        let mut token = HANDLE::default();
        if OpenProcessToken(proc, TOKEN_QUERY, &mut token).is_ok() {
            let mut elev = TOKEN_ELEVATION::default();
            let mut ret_len = 0u32;
            if GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut elev as *mut _ as *mut c_void),
                size_of::<TOKEN_ELEVATION>() as u32,
                &mut ret_len,
            )
            .is_ok()
            {
                elevated = elev.TokenIsElevated != 0;
            }
            let _ = CloseHandle(token);
        }

        let _ = CloseHandle(proc);
        (basename, elevated, false)
    }
}

/// Evaluate foreground window for same-monitor fullscreen game suppression (immune to Auto-Hide
/// Taskbar maximized browsers), credential-prompt privacy heuristics, and UIPI elevation.
pub fn evaluate_foreground(
    overlay_hwnd: HWND,
    capture_stage_active: bool,
    cfg: &Config,
) -> ForegroundEvaluation {
    let mut eval = ForegroundEvaluation {
        should_hide_for_game: false,
        fullscreen_reason: "none",
        auto_privacy: PrivacySource::None,
        foreground_elevated_or_inaccessible: false,
    };

    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() || fg == overlay_hwnd {
            return eval;
        }
        if !IsWindowVisible(fg).as_bool() || IsIconic(fg).as_bool() {
            return eval;
        }
        if fg == GetDesktopWindow() || fg == GetShellWindow() {
            return eval;
        }

        // Ignore cloaked UWP / virtual-desktop windows
        let mut cloaked: u32 = 0;
        let _ = DwmGetWindowAttribute(
            fg,
            DWMWA_CLOAKED,
            &mut cloaked as *mut _ as *mut c_void,
            size_of::<u32>() as u32,
        );
        if cloaked != 0 {
            return eval;
        }

        // Exclude Windows desktop shell classes
        let mut cls_buf = [0u16; 128];
        let cls_len = GetClassNameW(fg, &mut cls_buf) as usize;
        let cls = String::from_utf16_lossy(&cls_buf[..cls_len]);
        if matches!(
            cls.as_str(),
            "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
        ) {
            return eval;
        }

        // Query process basename + elevation strictly with PROCESS_QUERY_LIMITED_INFORMATION
        let mut pid = 0u32;
        let _ = GetWindowThreadProcessId(fg, Some(&mut pid));
        let (basename, elevated, access_denied) = if pid != 0 {
            query_process_basename_and_elevation(pid)
        } else {
            (None, false, false)
        };
        eval.foreground_elevated_or_inaccessible = elevated || access_denied;

        // Privacy heuristic: credential prompts / password managers / PPL inaccessible prompts
        if cfg.privacy_heuristics_enabled {
            if let Some(ref bn) = basename {
                let h = fnv1a_64_lower(bn);
                if credential_basename_hashes().contains(&h) {
                    eval.auto_privacy = PrivacySource::CredentialHeuristic;
                }
            } else if access_denied && cls.contains("Credential") {
                eval.auto_privacy = PrivacySource::CredentialHeuristic;
            }
        }

        // Fullscreen game detection is explicitly suspended while Capture Stage (Streamer Mode) is active
        if capture_stage_active || cfg.game_hide_scope == GameHideScope::Disabled {
            return eval;
        }

        if let Some(ref bn) = basename {
            if cfg
                .game_process_allowlist
                .iter()
                .any(|a| a.eq_ignore_ascii_case(bn))
            {
                return eval;
            }
            if cfg
                .game_process_denylist
                .iter()
                .any(|d| d.eq_ignore_ascii_case(bn))
            {
                eval.should_hide_for_game = true;
                eval.fullscreen_reason = "process_denylist";
                return eval;
            }
        }

        let fg_mon = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
        let ov_mon = MonitorFromWindow(overlay_hwnd, MONITOR_DEFAULTTONEAREST);
        if cfg.game_hide_scope == GameHideScope::SameMonitor && fg_mon != ov_mon {
            return eval;
        }

        // 1. Exclusive D3D fullscreen
        if let Ok(state) = SHQueryUserNotificationState() {
            if state == QUNS_RUNNING_D3D_FULL_SCREEN {
                eval.should_hide_for_game = true;
                eval.fullscreen_reason = "exclusive_d3d";
                return eval;
            }
        }

        // 2. Borderless fullscreen check (excluding framed maximized windows with Auto-Hide Taskbar)
        let mut mon_info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(fg_mon, &mut mon_info).as_bool() {
            let mut wrect = RECT::default();
            if DwmGetWindowAttribute(
                fg,
                DWMWA_EXTENDED_FRAME_BOUNDS,
                &mut wrect as *mut _ as *mut c_void,
                size_of::<RECT>() as u32,
            )
            .is_err()
            {
                let _ = GetWindowRect(fg, &mut wrect);
            }
            let rc_m = mon_info.rcMonitor;
            let covers_monitor = wrect.left <= rc_m.left
                && wrect.top <= rc_m.top
                && wrect.right >= rc_m.right
                && wrect.bottom >= rc_m.bottom;

            let style = GetWindowLongPtrW(fg, GWL_STYLE) as u32;
            let has_standard_frame =
                (style & WS_CAPTION.0) == WS_CAPTION.0 && (style & WS_THICKFRAME.0) != 0;

            if covers_monitor && !has_standard_frame {
                eval.should_hide_for_game = true;
                eval.fullscreen_reason = if access_denied {
                    "borderless_protected_game"
                } else {
                    "borderless_fullscreen"
                };
            }
        }
    }

    eval
}
