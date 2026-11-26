use crate::logging::log_info;
use pet_core::{BackdropMode, CatProfile, Rgb8, SimSnapshot};
use pet_render::{RenderOptions, Renderer};
use std::mem::size_of;
use windows::core::{PCWSTR, w};
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, GetDC, ReleaseDC, SRCCOPY, StretchDIBits,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, LoadCursorW, RegisterClassExW,
    ShowWindow, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, IDC_ARROW, SW_SHOWNOACTIVATE,
    WINDOW_EX_STYLE, WNDCLASSEXW, WM_CLOSE, WS_OVERLAPPEDWINDOW,
};

/// Separate non-topmost standard `WS_OVERLAPPEDWINDOW` ("Desktop Cat — Capture Stage")
/// without `WS_EX_TOOLWINDOW` or `WS_EX_LAYERED` so both OBS WGC and BitBlt discover and capture it cleanly.
pub struct CaptureStageWindow {
    hwnd: HWND,
    renderer: Renderer,
    bgra_buf: Vec<u8>,
    closed_by_user: bool,
}

static mut STAGE_CLOSED_FLAG: bool = false;

impl CaptureStageWindow {
    pub fn open() -> Option<Self> {
        unsafe {
            STAGE_CLOSED_FLAG = false;
            let hinst: HINSTANCE = GetModuleHandleW(PCWSTR::null()).unwrap_or_default().into();
            let class_name = w!("DesktopCatCaptureStage");
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(capture_stage_wndproc),
                hInstance: hinst,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                lpszClassName: class_name,
                ..Default::default()
            };
            let _ = RegisterClassExW(&wc);

            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class_name,
                w!("Desktop Cat - Capture Stage"),
                WS_OVERLAPPEDWINDOW,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                540,
                470,
                None,
                None,
                hinst,
                None,
            )
            .ok()?;

            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            log_info("capture_stage", "opened OBS Capture Stage window");
            Some(Self {
                hwnd,
                renderer: Renderer::new(540, 450),
                bgra_buf: Vec::new(),
                closed_by_user: false,
            })
        }
    }

    pub fn was_closed_by_user(&mut self) -> bool {
        unsafe {
            if STAGE_CLOSED_FLAG {
                self.closed_by_user = true;
            }
        }
        self.closed_by_user
    }

    pub fn render_and_blit(
        &mut self,
        profile: &CatProfile,
        sim: &SimSnapshot,
        chroma_rgb: Rgb8,
        hard_edge: bool,
        show_legends: bool,
    ) {
        unsafe {
            let mut rc = RECT::default();
            if GetClientRect(self.hwnd, &mut rc).is_err() {
                return;
            }
            let cw = (rc.right - rc.left).max(64) as u32;
            let ch = (rc.bottom - rc.top).max(64) as u32;
            let rw = cw.min(768);
            let rh = ch.min(768);
            self.renderer.resize(rw, rh);
            self.renderer.render(
                profile,
                sim,
                RenderOptions {
                    backdrop: BackdropMode::Solid(chroma_rgb),
                    chroma_hard_edge: hard_edge,
                    show_keycap_legends: show_legends,
                },
            );

            let src = self.renderer.rgba_bytes();
            self.bgra_buf.resize(src.len(), 0);
            for (d, s) in self.bgra_buf.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                d[0] = s[2];
                d[1] = s[1];
                d[2] = s[0];
                d[3] = 255;
            }

            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: rw as i32,
                    biHeight: -(rh as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };

            let hdc = GetDC(self.hwnd);
            if !hdc.is_invalid() {
                let _ = StretchDIBits(
                    hdc,
                    0,
                    0,
                    cw as i32,
                    ch as i32,
                    0,
                    0,
                    rw as i32,
                    rh as i32,
                    Some(self.bgra_buf.as_ptr() as *const _),
                    &bmi,
                    DIB_RGB_COLORS,
                    SRCCOPY,
                );
                let _ = ReleaseDC(self.hwnd, hdc);
            }
        }
    }
}

impl Drop for CaptureStageWindow {
    fn drop(&mut self) {
        unsafe {
            if !self.hwnd.0.is_null() {
                let _ = DestroyWindow(self.hwnd);
            }
        }
    }
}

unsafe extern "system" fn capture_stage_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_CLOSE {
        STAGE_CLOSED_FLAG = true;
        return LRESULT(0);
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}
