use crate::logging::log_warn;
use std::ffi::c_void;
use std::mem::size_of;
use windows::Win32::Foundation::{COLORREF, GetLastError, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, HBITMAP, HDC,
    HGDIOBJ, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    ULW_ALPHA, UPDATELAYEREDWINDOWINFO, UpdateLayeredWindowIndirect,
};

/// Persistent RAII top-down 32-bit premultiplied BGRA DIB surface for `UpdateLayeredWindowIndirect`.
/// Zero per-frame GDI allocations; restores old bitmap and releases DC + HBITMAP on drop.
pub struct LayeredSurface {
    hdc: HDC,
    hbitmap: HBITMAP,
    old_bmp: HGDIOBJ,
    bits: *mut u8,
    width: u32,
    height: u32,
}

impl LayeredSurface {
    pub fn new(width: u32, height: u32) -> Result<Self, String> {
        let w = width.max(1);
        let h = height.max(1);
        unsafe {
            let hdc = CreateCompatibleDC(HDC::default());
            if hdc.is_invalid() {
                return Err(format!("CreateCompatibleDC failed: {:?}", GetLastError()));
            }
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w as i32,
                    // Negative height => top-down DIB matching `tiny-skia`'s top-left origin!
                    biHeight: -(h as i32),
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let hbitmap = CreateDIBSection(hdc, &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
                .map_err(|e| {
                    let _ = DeleteDC(hdc);
                    format!("CreateDIBSection failed: {e}")
                })?;
            if bits.is_null() {
                let _ = DeleteObject(hbitmap);
                let _ = DeleteDC(hdc);
                return Err("CreateDIBSection returned null bits".to_string());
            }
            let old_bmp = SelectObject(hdc, hbitmap);
            Ok(Self {
                hdc,
                hbitmap,
                old_bmp,
                bits: bits as *mut u8,
                width: w,
                height: h,
            })
        }
    }

    pub fn resize_if_needed(&mut self, width: u32, height: u32) -> Result<bool, String> {
        if self.width == width && self.height == height {
            return Ok(false);
        }
        *self = Self::new(width, height)?;
        Ok(true)
    }

    /// Copy-swizzle premultiplied RGBA8 (`tiny-skia`) into premultiplied BGRA8 DIB,
    /// snapping faint shadow pixels (`alpha < 8`) to exact `0` so unlocked native `alpha == 0`
    /// click pass-through has zero invisible click-traps, then present via `UpdateLayeredWindowIndirect`.
    pub fn present_rgba(
        &mut self,
        hwnd: HWND,
        pt_dst: Option<POINT>,
        rgba_premul: &[u8],
        opacity: f32,
        dirty_rect: Option<RECT>,
    ) -> bool {
        let expected = (self.width as usize) * (self.height as usize) * 4;
        if rgba_premul.len() != expected {
            return false;
        }
        unsafe {
            let dst_slice = std::slice::from_raw_parts_mut(self.bits, expected);
            // Auto-vectorizable RGBA -> BGRA swizzle with alpha < 8 fringe snap
            for (dst, src) in dst_slice.chunks_exact_mut(4).zip(rgba_premul.chunks_exact(4)) {
                let a = src[3];
                if a < 8 {
                    dst[0] = 0;
                    dst[1] = 0;
                    dst[2] = 0;
                    dst[3] = 0;
                } else {
                    // Enforce premultiplied invariant (R,G,B <= A) to prevent DWM neon halos
                    dst[0] = src[2].min(a);
                    dst[1] = src[1].min(a);
                    dst[2] = src[0].min(a);
                    dst[3] = a;
                }
            }

            let alpha_byte = (opacity.clamp(0.05, 1.0) * 255.0).round() as u8;
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: alpha_byte,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let pt_src = POINT { x: 0, y: 0 };
            let size = SIZE {
                cx: self.width as i32,
                cy: self.height as i32,
            };
            let dst_ptr = pt_dst
                .as_ref()
                .map(|p| p as *const POINT)
                .unwrap_or(std::ptr::null());
            let dirty_ptr = dirty_rect
                .as_ref()
                .map(|r| r as *const RECT)
                .unwrap_or(std::ptr::null());

            let info = UPDATELAYEREDWINDOWINFO {
                cbSize: size_of::<UPDATELAYEREDWINDOWINFO>() as u32,
                hdcDst: HDC::default(),
                pptDst: dst_ptr,
                psize: &size,
                hdcSrc: self.hdc,
                pptSrc: &pt_src,
                crKey: COLORREF(0),
                pblend: &blend,
                dwFlags: ULW_ALPHA,
                prcDirty: dirty_ptr,
            };

            if !UpdateLayeredWindowIndirect(hwnd, &info).as_bool() {
                log_warn(
                    "surface",
                    &format!("UpdateLayeredWindowIndirect failed: {:?}", GetLastError()),
                );
                return false;
            }
            true
        }
    }
}

impl Drop for LayeredSurface {
    fn drop(&mut self) {
        unsafe {
            if !self.hdc.is_invalid() {
                if !self.old_bmp.is_invalid() {
                    let _ = SelectObject(self.hdc, self.old_bmp);
                }
                if !self.hbitmap.is_invalid() {
                    let _ = DeleteObject(self.hbitmap);
                }
                let _ = DeleteDC(self.hdc);
            }
        }
    }
}
