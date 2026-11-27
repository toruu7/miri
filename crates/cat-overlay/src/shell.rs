use crate::logging::{log_info, log_warn};
use crate::monitor::MonitorEntry;
use pet_core::{builtin_presets, CatProfile, Config, HotkeyConfig};
use std::ffi::OsStr;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use windows::core::{PCWSTR, w};
use windows::Win32::Foundation::{GetLastError, HANDLE, HINSTANCE, HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
    DeleteObject, HBITMAP, HDC,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, RegisterHotKey,
    UnregisterHotKey,
};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
    Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, ChangeWindowMessageFilterEx, CreateIconIndirect, CreatePopupMenu, CreateWindowExW,
    DestroyIcon, DestroyMenu, DestroyWindow, GetCursorPos, HICON, HMENU, ICONINFO, MF_CHECKED,
    MF_POPUP, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MSGFLT_ALLOW, PostMessageW,
    RegisterClassExW, RegisterWindowMessageW, SetForegroundWindow, TPM_BOTTOMALIGN, TPM_LEFTALIGN,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenuEx, WINDOW_EX_STYLE, WM_APP, WM_NULL,
    WNDCLASSEXW, WNDPROC, WS_POPUP,
};

pub const WM_APP_TRAY: u32 = WM_APP + 1;
pub const WM_APP_SUMMON_ESCAPE: u32 = WM_APP + 2;

pub const HOTKEY_ID_CLICK_THROUGH: i32 = 1;
pub const HOTKEY_ID_PRIVACY: i32 = 2;
pub const HOTKEY_ID_DESIGNER: i32 = 3;

pub const CMD_OPEN_DESIGNER: u32 = 1001;
pub const CMD_TOGGLE_LOCK: u32 = 1002;
pub const CMD_TOGGLE_PRIVACY: u32 = 1003;
pub const CMD_TOGGLE_SHOWCASE: u32 = 1004;
pub const CMD_TOGGLE_POPUPS: u32 = 1005;
pub const CMD_TOGGLE_CAPTURE_STAGE: u32 = 1006;
pub const CMD_RESET_POS: u32 = 1007;
pub const CMD_COPY_DIAG: u32 = 1008;
pub const CMD_OPEN_CONFIG_DIR: u32 = 1009;
pub const CMD_TOGGLE_AUTOSTART: u32 = 1010;
pub const CMD_SNAP_BR: u32 = 1011;
pub const CMD_SNAP_BL: u32 = 1012;
pub const CMD_SNAP_TR: u32 = 1013;
pub const CMD_SNAP_TL: u32 = 1014;
pub const CMD_SCALE_MINI: u32 = 1015;
pub const CMD_SCALE_SMALL: u32 = 1016;
pub const CMD_SCALE_NORMAL: u32 = 1017;
pub const CMD_SCALE_LARGE: u32 = 1018;
pub const CMD_QUIT: u32 = 1099;
pub const CMD_PRESET_BASE: u32 = 2000;
pub const CMD_USER_PROFILE_BASE: u32 = 3000;
pub const CMD_MONITOR_BASE: u32 = 4000;

const TRAY_UID: u32 = 0xCA71;

pub struct HelperShell {
    pub hwnd: HWND,
    pub taskbar_created_msg: u32,
    hicon: HICON,
    tray_added: bool,
}

impl HelperShell {
    /// Create a 0×0 invisible top-level helper HWND (`WS_POPUP`, `hwndParent = NULL` — NOT `HWND_MESSAGE`)
    /// so it receives `TaskbarCreated` broadcasts, owns tray + hotkeys, and hosts KB135788-safe menus
    /// without activating the `WS_EX_NOACTIVATE` overlay.
    pub fn create(hinst: HINSTANCE, wndproc: WNDPROC) -> Result<Self, String> {
        unsafe {
            let class_name = w!("DesktopCatHelperHost");
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: wndproc,
                hInstance: hinst,
                lpszClassName: class_name,
                ..Default::default()
            };
            let _ = RegisterClassExW(&wc);

            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class_name,
                w!("DesktopCatHelperHost"),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                hinst,
                None,
            )
            .map_err(|e| format!("create helper hwnd: {e}"))?;

            let taskbar_created_msg = RegisterWindowMessageW(w!("TaskbarCreated"));
            if taskbar_created_msg != 0 {
                let _ = ChangeWindowMessageFilterEx(hwnd, taskbar_created_msg, MSGFLT_ALLOW, None);
            }
            let _ = ChangeWindowMessageFilterEx(hwnd, WM_APP_SUMMON_ESCAPE, MSGFLT_ALLOW, None);

            let hicon = create_cat_tray_icon();
            let mut shell = Self {
                hwnd,
                taskbar_created_msg,
                hicon,
                tray_added: false,
            };
            shell.add_or_restore_tray("Desktop Cat Companion");
            Ok(shell)
        }
    }

    pub fn add_or_restore_tray(&mut self, tooltip: &str) {
        unsafe {
            let mut nid = NOTIFYICONDATAW {
                cbSize: size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: self.hwnd,
                uID: TRAY_UID,
                uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
                uCallbackMessage: WM_APP_TRAY,
                hIcon: self.hicon,
                ..Default::default()
            };
            write_wide_fixed(&mut nid.szTip, tooltip);
            if Shell_NotifyIconW(NIM_ADD, &nid).as_bool() {
                self.tray_added = true;
            } else {
                let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
                self.tray_added = true;
            }
        }
    }

    pub fn update_tooltip(&self, tooltip: &str) {
        if !self.tray_added {
            return;
        }
        unsafe {
            let mut nid = NOTIFYICONDATAW {
                cbSize: size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: self.hwnd,
                uID: TRAY_UID,
                uFlags: NIF_TIP,
                ..Default::default()
            };
            write_wide_fixed(&mut nid.szTip, tooltip);
            let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    pub fn remove_tray(&mut self) {
        if !self.tray_added {
            return;
        }
        unsafe {
            let nid = NOTIFYICONDATAW {
                cbSize: size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: self.hwnd,
                uID: TRAY_UID,
                ..Default::default()
            };
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
            self.tray_added = false;
        }
    }

    pub fn register_hotkeys(&self, hk: &HotkeyConfig) -> Vec<String> {
        let mut conflicts = Vec::new();
        unsafe {
            let _ = UnregisterHotKey(self.hwnd, HOTKEY_ID_CLICK_THROUGH);
            let _ = UnregisterHotKey(self.hwnd, HOTKEY_ID_PRIVACY);
            let _ = UnregisterHotKey(self.hwnd, HOTKEY_ID_DESIGNER);

            for (id, label, spec) in [
                (HOTKEY_ID_CLICK_THROUGH, "click_through", &hk.toggle_click_through),
                (HOTKEY_ID_PRIVACY, "privacy", &hk.toggle_privacy),
                (HOTKEY_ID_DESIGNER, "designer", &hk.open_designer),
            ] {
                if let Some((mods, vk)) = parse_hotkey(spec) {
                    if let Err(e) = RegisterHotKey(self.hwnd, id, mods | MOD_NOREPEAT, vk) {
                        let msg = format!("{label} ({spec}): err {:?}", GetLastError());
                        log_warn("hotkey", &format!("RegisterHotKey failed: {msg} ({e})"));
                        conflicts.push(msg);
                    }
                }
            }
        }
        conflicts
    }

    /// KB135788-safe context menu hosted on the helper HWND (never activates the overlay).
    pub fn show_context_menu(
        &self,
        cfg: &Config,
        manual_privacy: bool,
        user_profiles: &[CatProfile],
        monitors: &[MonitorEntry],
        current_monitor: &str,
    ) -> u32 {
        unsafe {
            let Ok(hmenu) = CreatePopupMenu() else {
                return 0;
            };
            append_item(hmenu, CMD_OPEN_DESIGNER, "Open Cat Designer...", false);
            append_sep(hmenu);
            append_item(
                hmenu,
                CMD_TOGGLE_LOCK,
                &format!("Click-Through Lock ({})", cfg.hotkeys.toggle_click_through),
                cfg.click_through_locked,
            );
            append_item(
                hmenu,
                CMD_TOGGLE_PRIVACY,
                &format!("Privacy Mode ({})", cfg.hotkeys.toggle_privacy),
                manual_privacy,
            );
            append_item(
                hmenu,
                CMD_TOGGLE_SHOWCASE,
                "Showcase Mode (Exact 1:1 Keys)",
                cfg.showcase_mode,
            );
            append_item(
                hmenu,
                CMD_TOGGLE_POPUPS,
                "Floating Keycap Pop-Ups (Showcase)",
                cfg.show_floating_popups,
            );
            append_item(
                hmenu,
                CMD_TOGGLE_CAPTURE_STAGE,
                "OBS Capture Stage (Green Screen)",
                cfg.capture_stage_enabled,
            );
            append_sep(hmenu);

            // Built-in 15 presets submenu
            if let Ok(sub) = CreatePopupMenu() {
                for (i, p) in builtin_presets().iter().enumerate() {
                    append_item(
                        sub,
                        CMD_PRESET_BASE + i as u32,
                        &p.name,
                        cfg.active_profile_id == p.id,
                    );
                }
                append_submenu(hmenu, sub, "Built-In Coat Presets");
            }

            if !user_profiles.is_empty() {
                if let Ok(sub) = CreatePopupMenu() {
                    for (i, p) in user_profiles.iter().enumerate().take(60) {
                        append_item(
                            sub,
                            CMD_USER_PROFILE_BASE + i as u32,
                            &p.name,
                            cfg.active_profile_id == p.id,
                        );
                    }
                    append_submenu(hmenu, sub, "Saved Custom Profiles");
                }
            }

            append_sep(hmenu);

            // Quick multi-monitor, snap, and scale submenus (zero-RAM daily-driver controls)
            if !monitors.is_empty() {
                if let Ok(sub) = CreatePopupMenu() {
                    for (i, m) in monitors.iter().enumerate().take(16) {
                        let prim = if m.is_primary { " Primary," } else { "" };
                        let lbl = format!("Display {} ({prim} {}x{})", i + 1, m.width, m.height);
                        let is_cur = m.device_name.eq_ignore_ascii_case(current_monitor);
                        append_item(sub, CMD_MONITOR_BASE + i as u32, &lbl, is_cur);
                    }
                    append_submenu(hmenu, sub, "Move to Monitor");
                }
            }
            if let Ok(sub) = CreatePopupMenu() {
                append_item(sub, CMD_SNAP_BR, "Bottom-Right", false);
                append_item(sub, CMD_SNAP_BL, "Bottom-Left", false);
                append_item(sub, CMD_SNAP_TR, "Top-Right", false);
                append_item(sub, CMD_SNAP_TL, "Top-Left", false);
                append_submenu(hmenu, sub, "Snap Position");
            }
            if let Ok(sub) = CreatePopupMenu() {
                append_item(sub, CMD_SCALE_MINI, "Mini (0.6x)", (cfg.scale - 0.6).abs() < 0.05);
                append_item(sub, CMD_SCALE_SMALL, "Small (0.85x)", (cfg.scale - 0.85).abs() < 0.05);
                append_item(sub, CMD_SCALE_NORMAL, "Normal (1.0x)", (cfg.scale - 1.0).abs() < 0.05);
                append_item(sub, CMD_SCALE_LARGE, "Large (1.5x)", (cfg.scale - 1.5).abs() < 0.05);
                append_submenu(hmenu, sub, "Scale");
            }

            append_item(hmenu, CMD_TOGGLE_AUTOSTART, "Auto-Start on Login", cfg.auto_start);
            append_item(hmenu, CMD_RESET_POS, "Reset Position to Primary Screen", false);
            append_item(hmenu, CMD_COPY_DIAG, "Copy Diagnostics Report", false);
            append_item(hmenu, CMD_OPEN_CONFIG_DIR, "Open Config Folder", false);
            append_sep(hmenu);
            append_item(hmenu, CMD_QUIT, "Quit Desktop Cat", false);

            let mut pt = POINT::default();
            let _ = GetCursorPos(&mut pt);
            let _ = SetForegroundWindow(self.hwnd);
            let cmd = TrackPopupMenuEx(
                hmenu,
                (TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_LEFTALIGN | TPM_BOTTOMALIGN).0,
                pt.x,
                pt.y,
                self.hwnd,
                None,
            );
            let _ = PostMessageW(self.hwnd, WM_NULL, WPARAM(0), LPARAM(0));
            let _ = DestroyMenu(hmenu);
            cmd.0 as u32
        }
    }
}

impl Drop for HelperShell {
    fn drop(&mut self) {
        self.remove_tray();
        unsafe {
            let _ = UnregisterHotKey(self.hwnd, HOTKEY_ID_CLICK_THROUGH);
            let _ = UnregisterHotKey(self.hwnd, HOTKEY_ID_PRIVACY);
            let _ = UnregisterHotKey(self.hwnd, HOTKEY_ID_DESIGNER);
            if !self.hicon.is_invalid() {
                let _ = DestroyIcon(self.hicon);
            }
            if !self.hwnd.0.is_null() {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

pub fn copy_text_to_clipboard(owner: HWND, text: &str) -> bool {
    unsafe {
        if OpenClipboard(owner).is_err() {
            return false;
        }
        let _ = EmptyClipboard();
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = wide.len() * 2;
        let Ok(hmem) = GlobalAlloc(GMEM_MOVEABLE, bytes) else {
            let _ = CloseClipboard();
            return false;
        };
        let ptr = GlobalLock(hmem) as *mut u16;
        if !ptr.is_null() {
            std::ptr::copy_nonoverlapping(wide.as_ptr(), ptr, wide.len());
            let _ = GlobalUnlock(hmem);
            // CF_UNICODETEXT = 13
            let _ = SetClipboardData(13, HANDLE(hmem.0));
        }
        let _ = CloseClipboard();
        log_info("diagnostics", "copied redacted diagnostics JSON to clipboard");
        true
    }
}

fn wide_z(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

fn write_wide_fixed(dst: &mut [u16], s: &str) {
    dst.fill(0);
    let max_len = dst.len().saturating_sub(1);
    for (slot, ch) in dst.iter_mut().take(max_len).zip(s.encode_utf16()) {
        *slot = ch;
    }
}

unsafe fn append_item(menu: HMENU, id: u32, label: &str, checked: bool) {
    let w = wide_z(label);
    let flags = MF_STRING | if checked { MF_CHECKED } else { MF_UNCHECKED };
    let _ = AppendMenuW(menu, flags, id as usize, PCWSTR(w.as_ptr()));
}

unsafe fn append_submenu(menu: HMENU, sub: HMENU, label: &str) {
    let w = wide_z(label);
    let _ = AppendMenuW(menu, MF_POPUP, sub.0 as usize, PCWSTR(w.as_ptr()));
}

unsafe fn append_sep(menu: HMENU) {
    let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
}

fn parse_hotkey(spec: &str) -> Option<(HOT_KEY_MODIFIERS, u32)> {
    let mut mods = HOT_KEY_MODIFIERS(0);
    let mut vk = 0u32;
    for part in spec.split('+') {
        match part.trim().to_ascii_uppercase().as_str() {
            "CTRL" | "CONTROL" => mods |= MOD_CONTROL,
            "ALT" => mods |= MOD_ALT,
            "SHIFT" => mods |= MOD_SHIFT,
            s if s.len() == 1 => {
                let b = s.as_bytes()[0];
                if b.is_ascii_alphanumeric() {
                    vk = b as u32;
                }
            }
            _ => {}
        }
    }
    if vk == 0 {
        None
    } else {
        Some((mods, vk))
    }
}

/// Draw a crisp 16×16 cat silhouette tray icon (black head + ears + glowing green eyes).
unsafe fn create_cat_tray_icon() -> HICON {
    let w = 16i32;
    let h = 16i32;
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
    let Ok(color_bmp) = CreateDIBSection(HDC::default(), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
    else {
        return HICON::default();
    };

    if !bits.is_null() {
        let px = std::slice::from_raw_parts_mut(bits as *mut u8, (w * h * 4) as usize);
        px.fill(0);
        let set = |px: &mut [u8], x: i32, y: i32, r: u8, g: u8, b: u8, a: u8| {
            if (0..16).contains(&x) && (0..16).contains(&y) {
                let i = ((y * 16 + x) * 4) as usize;
                px[i] = b;
                px[i + 1] = g;
                px[i + 2] = r;
                px[i + 3] = a;
            }
        };
        // Cat silhouette head + ears
        for y in 2..15 {
            for x in 1..15 {
                let dx = x as f32 - 7.5;
                let dy = y as f32 - 9.5;
                let in_head = (dx * dx) / 36.0 + (dy * dy) / 22.0 <= 1.0;
                let in_l_ear = (2..=7).contains(&y) && (2..=5).contains(&x) && (x - 2) <= (y - 2);
                let in_r_ear = (2..=7).contains(&y) && (10..=13).contains(&x) && (13 - x) <= (y - 2);
                if in_head || in_l_ear || in_r_ear {
                    set(px, x, y, 22, 22, 28, 255);
                }
            }
        }
        // Bright green cat eyes
        for (ex, ey) in [(5, 9), (6, 9), (5, 10), (10, 9), (11, 9), (10, 10)] {
            set(px, ex, ey, 200, 240, 77, 255);
        }
        // Tiny dark pupils
        set(px, 6, 10, 15, 15, 20, 255);
        set(px, 11, 10, 15, 15, 20, 255);
    }

    let mask_bmp: HBITMAP = CreateBitmap(w, h, 1, 1, None);
    let ii = ICONINFO {
        fIcon: true.into(),
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: mask_bmp,
        hbmColor: color_bmp,
    };
    let icon = CreateIconIndirect(&ii).unwrap_or_default();
    let _ = DeleteObject(color_bmp);
    let _ = DeleteObject(mask_bmp);
    icon
}
