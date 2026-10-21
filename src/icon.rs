//! Icons. The app and tray icons are compiled into the exe (resources.rc,
//! drawn by tools/make_icon.py). If they are missing, a plain coloured dot is
//! drawn at runtime instead, so the app never runs without an icon.

use crate::analyze::Level;
use windows_sys::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateIconIndirect, GetSystemMetrics, LoadImageW, HICON, ICONINFO,
};

const IDI_APP: usize = 1;
const IMAGE_ICON: u32 = 1;

pub fn level_rgb(level: Level) -> (u8, u8, u8) {
    match level {
        Level::Normal => (34, 197, 94),
        Level::High => (245, 158, 11),
        Level::Critical => (239, 68, 68),
    }
}

fn load(id: usize, size: i32) -> HICON {
    unsafe { LoadImageW(GetModuleHandleW(std::ptr::null()), id as *const u16, IMAGE_ICON, size, size, 0) as HICON }
}

fn metric(large: bool) -> i32 {
    unsafe { GetSystemMetrics(if large { 11 } else { 49 }) }.clamp(16, 256) // SM_CXICON / SM_CXSMICON
}

/// The app icon at the system small/large size (or a custom pixel size).
pub fn app_icon(size: i32) -> HICON {
    let h = load(IDI_APP, size);
    if h.is_null() {
        status_dot(Level::Normal, size)
    } else {
        h
    }
}

pub fn app_icon_sized(large: bool) -> HICON {
    app_icon(metric(large))
}

/// Tray icon: app icon with a green / amber / red status dot.
pub fn tray_icon(level: Level) -> HICON {
    let id = match level {
        Level::Normal => 2,
        Level::High => 3,
        Level::Critical => 4,
    };
    let size = metric(false);
    let h = load(id, size);
    if h.is_null() {
        status_dot(level, size)
    } else {
        h
    }
}

/// BGRA pixels + 1-bit AND mask for a filled circle with a darker rim.
pub fn circle_pixels(size: usize, rgb: (u8, u8, u8)) -> (Vec<u32>, Vec<u8>) {
    let stride = size.div_ceil(16) * 2; // mask rows are WORD aligned
    let mut color = vec![0u32; size * size];
    let mut mask = vec![0xFFu8; stride * size];
    let c = size as f32 / 2.0;
    let r = c - 0.5;
    let shade = |v: u8, f: f32| (v as f32 * f) as u32;
    for y in 0..size {
        for x in 0..size {
            let d = ((x as f32 + 0.5 - c).powi(2) + (y as f32 + 0.5 - c).powi(2)).sqrt();
            if d <= r {
                let f = if d > r - 1.5 { 0.7 } else { 1.0 };
                color[y * size + x] =
                    0xFF00_0000 | (shade(rgb.0, f) << 16) | (shade(rgb.1, f) << 8) | shade(rgb.2, f);
                mask[y * stride + x / 8] &= !(0x80 >> (x % 8));
            }
        }
    }
    (color, mask)
}

/// Fallback icon drawn at runtime.
pub fn status_dot(level: Level, size: i32) -> HICON {
    let size = size.clamp(16, 64) as usize;
    let (color, mask) = circle_pixels(size, level_rgb(level));
    unsafe {
        let hbm_color = CreateBitmap(size as i32, size as i32, 1, 32, color.as_ptr() as *const _);
        let hbm_mask = CreateBitmap(size as i32, size as i32, 1, 1, mask.as_ptr() as *const _);
        let info = ICONINFO { fIcon: 1, xHotspot: 0, yHotspot: 0, hbmMask: hbm_mask, hbmColor: hbm_color };
        let icon = CreateIconIndirect(&info);
        DeleteObject(hbm_color);
        DeleteObject(hbm_mask);
        icon
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_is_opaque_inside_transparent_outside() {
        let (color, mask) = circle_pixels(16, (10, 20, 30));
        let stride = 2;
        let inside = |x: usize, y: usize| mask[y * stride + x / 8] & (0x80 >> (x % 8)) == 0;
        assert!(inside(8, 8));
        assert_eq!(color[8 * 16 + 8], 0xFF0A141E);
        assert!(!inside(0, 0));
        assert_eq!(color[0], 0);
    }
}
