//! Shared drawing for the window and the tray popup: palette, fonts, GDI text,
//! GDI+ anti-aliased shapes, and hover tracking for owner-drawn buttons.

use crate::analyze::Level;
use crate::util::wide;
use std::cell::Cell;
use std::ffi::c_void;
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, SIZE, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, FillRect, GetTextExtentPoint32W, InvalidateRect,
    SelectObject, SetBkMode, SetTextColor, HDC, HFONT,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TRACKMOUSEEVENT};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{LoadCursorW, SetCursor, IDC_HAND};

pub type Rgb = (u8, u8, u8);

pub const BG: Rgb = (241, 245, 249);
pub const CARD: Rgb = (255, 255, 255);
pub const BORDER: Rgb = (226, 232, 240);
pub const INK: Rgb = (15, 23, 42);
pub const MUTED: Rgb = (100, 116, 139);
pub const SIDEBAR: Rgb = (15, 23, 42);
pub const SIDEBAR_ACTIVE: Rgb = (30, 41, 59);
pub const SIDEBAR_HOVER: Rgb = (23, 33, 51);
pub const SIDEBAR_TEXT: Rgb = (203, 213, 225);
pub const SIDEBAR_MUTED: Rgb = (148, 163, 184);
pub const ACCENT: Rgb = (13, 148, 136);
pub const ACCENT_LIGHT: Rgb = (45, 212, 191);
pub const ACCENT_HOVER: Rgb = (15, 118, 110);
pub const ACCENT_DOWN: Rgb = (17, 94, 89);
pub const BLUE: Rgb = (37, 99, 235);
pub const DISABLED: Rgb = (203, 213, 225);
pub const GOOD: Rgb = (22, 163, 74);
pub const WARN: Rgb = (217, 119, 6);
pub const BAD: Rgb = (220, 38, 38);
pub const TRACK: Rgb = (226, 232, 240);
pub const WHITE: Rgb = (255, 255, 255);

pub const DT_CENTER: u32 = 0x1;
pub const DT_RIGHT: u32 = 0x2;
pub const DT_VCENTER: u32 = 0x4;
pub const DT_WORDBREAK: u32 = 0x10;
pub const DT_SINGLELINE: u32 = 0x20;
pub const DT_NOPREFIX: u32 = 0x800;
pub const DT_END_ELLIPSIS: u32 = 0x8000;

pub fn level_color(l: Level) -> Rgb {
    match l {
        Level::Normal => GOOD,
        Level::High => WARN,
        Level::Critical => BAD,
    }
}

pub fn score_color(score: u8) -> Rgb {
    match score {
        75.. => GOOD,
        50..=74 => WARN,
        _ => BAD,
    }
}

pub fn colorref(c: Rgb) -> u32 {
    c.0 as u32 | (c.1 as u32) << 8 | (c.2 as u32) << 16
}

pub fn argb(c: Rgb) -> u32 {
    0xFF00_0000 | (c.0 as u32) << 16 | (c.1 as u32) << 8 | c.2 as u32
}

pub fn rect(l: i32, t: i32, r: i32, b: i32) -> RECT {
    RECT { left: l, top: t, right: r, bottom: b }
}

// ------------------------------------------------------------------- GDI+

#[repr(C)]
struct GdiplusStartupInput {
    version: u32,
    callback: usize,
    suppress_background_thread: i32,
    suppress_external_codecs: i32,
}

#[repr(C)]
struct PointF {
    x: f32,
    y: f32,
}

type Gp = *mut c_void;

#[link(name = "gdiplus")]
extern "system" {
    fn GdiplusStartup(token: *mut usize, input: *const GdiplusStartupInput, output: *mut c_void) -> i32;
    fn GdipCreateFromHDC(hdc: HDC, g: *mut Gp) -> i32;
    fn GdipDeleteGraphics(g: Gp) -> i32;
    fn GdipSetSmoothingMode(g: Gp, mode: i32) -> i32;
    fn GdipCreateSolidFill(argb: u32, brush: *mut Gp) -> i32;
    fn GdipCreateLineBrush(p1: *const PointF, p2: *const PointF, c1: u32, c2: u32, wrap: i32, brush: *mut Gp) -> i32;
    fn GdipDeleteBrush(b: Gp) -> i32;
    fn GdipCreatePen1(argb: u32, width: f32, unit: i32, pen: *mut Gp) -> i32;
    fn GdipDeletePen(p: Gp) -> i32;
    fn GdipSetPenStartCap(p: Gp, cap: i32) -> i32;
    fn GdipSetPenEndCap(p: Gp, cap: i32) -> i32;
    fn GdipDrawArc(g: Gp, pen: Gp, x: f32, y: f32, w: f32, h: f32, start: f32, sweep: f32) -> i32;
    fn GdipCreatePath(fill_mode: i32, path: *mut Gp) -> i32;
    fn GdipDeletePath(p: Gp) -> i32;
    fn GdipAddPathArc(p: Gp, x: f32, y: f32, w: f32, h: f32, start: f32, sweep: f32) -> i32;
    fn GdipAddPathEllipse(p: Gp, x: f32, y: f32, w: f32, h: f32) -> i32;
    fn GdipClosePathFigure(p: Gp) -> i32;
    fn GdipFillPath(g: Gp, brush: Gp, path: Gp) -> i32;
    fn GdipDrawPath(g: Gp, pen: Gp, path: Gp) -> i32;
}

/// Starts GDI+ once per process.
pub fn startup() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        let mut token = 0usize;
        let input = GdiplusStartupInput { version: 1, callback: 0, suppress_background_thread: 0, suppress_external_codecs: 0 };
        GdiplusStartup(&mut token, &input, null_mut());
    });
}

/// Anti-aliased shapes on a GDI device context.
pub struct Gfx(Gp);

impl Gfx {
    /// # Safety
    /// `hdc` must be a valid device context for the lifetime of the value.
    pub unsafe fn new(hdc: HDC) -> Gfx {
        let mut g = null_mut();
        GdipCreateFromHDC(hdc, &mut g);
        GdipSetSmoothingMode(g, 4); // anti-alias
        Gfx(g)
    }

    unsafe fn round_path(x: f32, y: f32, w: f32, h: f32, r: f32) -> Gp {
        let mut p = null_mut();
        GdipCreatePath(0, &mut p);
        let d = (r * 2.0).min(w).min(h).max(1.0);
        GdipAddPathArc(p, x, y, d, d, 180.0, 90.0);
        GdipAddPathArc(p, x + w - d, y, d, d, 270.0, 90.0);
        GdipAddPathArc(p, x + w - d, y + h - d, d, d, 0.0, 90.0);
        GdipAddPathArc(p, x, y + h - d, d, d, 90.0, 90.0);
        GdipClosePathFigure(p);
        p
    }

    fn dims(rc: &RECT) -> (f32, f32, f32, f32) {
        (rc.left as f32, rc.top as f32, (rc.right - rc.left) as f32, (rc.bottom - rc.top) as f32)
    }

    unsafe fn fill_path(&self, path: Gp, fill: Rgb, fill2: Option<Rgb>, from: (f32, f32), to: (f32, f32)) {
        let mut b = null_mut();
        match fill2 {
            Some(c2) => {
                let (p1, p2) = (PointF { x: from.0, y: from.1 }, PointF { x: to.0, y: to.1 });
                GdipCreateLineBrush(&p1, &p2, argb(fill), argb(c2), 0, &mut b);
            }
            None => {
                GdipCreateSolidFill(argb(fill), &mut b);
            }
        }
        GdipFillPath(self.0, b, path);
        GdipDeleteBrush(b);
    }

    pub unsafe fn fill_round(&self, rc: &RECT, r: f32, fill: Rgb, border: Option<Rgb>) {
        let (x, y, w, h) = Self::dims(rc);
        let path = Self::round_path(x + 0.5, y + 0.5, w - 1.0, h - 1.0, r);
        self.fill_path(path, fill, None, (0.0, 0.0), (0.0, 0.0));
        if let Some(bc) = border {
            let mut pen = null_mut();
            GdipCreatePen1(argb(bc), 1.0, 2, &mut pen);
            GdipDrawPath(self.0, pen, path);
            GdipDeletePen(pen);
        }
        GdipDeletePath(path);
    }

    /// Rounded rectangle with a diagonal gradient from `c1` to `c2`.
    pub unsafe fn fill_round_gradient(&self, rc: &RECT, r: f32, c1: Rgb, c2: Rgb) {
        let (x, y, w, h) = Self::dims(rc);
        let path = Self::round_path(x + 0.5, y + 0.5, w - 1.0, h - 1.0, r);
        self.fill_path(path, c1, Some(c2), (x - 1.0, y - 1.0), (x + w + 1.0, y + h + 1.0));
        GdipDeletePath(path);
    }

    pub unsafe fn circle(&self, cx: f32, cy: f32, r: f32, c1: Rgb, c2: Option<Rgb>) {
        let mut p = null_mut();
        GdipCreatePath(0, &mut p);
        GdipAddPathEllipse(p, cx - r, cy - r, r * 2.0, r * 2.0);
        self.fill_path(p, c1, c2, (cx - r, cy - r), (cx + r, cy + r));
        GdipDeletePath(p);
    }

    pub unsafe fn arc(&self, cx: f32, cy: f32, radius: f32, width: f32, start: f32, sweep: f32, color: Rgb) {
        let mut pen = null_mut();
        GdipCreatePen1(argb(color), width, 2, &mut pen);
        GdipSetPenStartCap(pen, 2); // round
        GdipSetPenEndCap(pen, 2);
        GdipDrawArc(self.0, pen, cx - radius, cy - radius, radius * 2.0, radius * 2.0, start, sweep);
        GdipDeletePen(pen);
    }

    pub unsafe fn dot(&self, cx: f32, cy: f32, r: f32, color: Rgb) {
        self.circle(cx, cy, r, color, None);
    }
}

impl Drop for Gfx {
    fn drop(&mut self) {
        unsafe {
            GdipDeleteGraphics(self.0);
        }
    }
}

// -------------------------------------------------------------------- GDI

/// # Safety
/// `hdc` must be a valid device context.
pub unsafe fn fill(hdc: HDC, rc: &RECT, c: Rgb) {
    let b = CreateSolidBrush(colorref(c));
    FillRect(hdc, rc, b);
    DeleteObject(b);
}

/// # Safety
/// `hdc` must be a valid device context and `font` a valid font.
pub unsafe fn text(hdc: HDC, s: &str, rc: RECT, font: HFONT, color: Rgb, flags: u32) {
    SelectObject(hdc, font);
    SetTextColor(hdc, colorref(color));
    SetBkMode(hdc, 1);
    let w = wide(s);
    let mut r = rc;
    DrawTextW(hdc, w.as_ptr(), -1, &mut r, flags | DT_NOPREFIX);
}

/// # Safety
/// Plain Win32 call.
pub unsafe fn font(dpi: i32, points: i32, weight: i32, face: &str) -> HFONT {
    CreateFontW(-(points * dpi / 72), 0, 0, 0, weight, 0, 0, 0, 1, 0, 0, 5, 0, wide(face).as_ptr())
}

#[derive(Clone, Copy)]
pub struct Fonts {
    pub body: HFONT,
    pub bold: HFONT,
    pub small: HFONT,
    pub small_bold: HFONT,
    pub section: HFONT,
    pub title: HFONT,
    pub big: HFONT,
    pub huge: HFONT,
    pub brand: HFONT,
    pub icons: HFONT,
    pub icons_big: HFONT,
}

impl Fonts {
    /// # Safety
    /// Plain Win32 calls.
    pub unsafe fn new(dpi: i32) -> Fonts {
        Fonts {
            body: font(dpi, 10, 400, "Segoe UI"),
            bold: font(dpi, 10, 600, "Segoe UI"),
            small: font(dpi, 9, 400, "Segoe UI"),
            small_bold: font(dpi, 9, 600, "Segoe UI"),
            section: font(dpi, 12, 600, "Segoe UI"),
            title: font(dpi, 18, 600, "Segoe UI"),
            big: font(dpi, 22, 700, "Segoe UI"),
            huge: font(dpi, 30, 700, "Segoe UI"),
            brand: font(dpi, 13, 700, "Segoe UI"),
            icons: font(dpi, 12, 400, "Segoe Fluent Icons"),
            icons_big: font(dpi, 30, 400, "Segoe Fluent Icons"),
        }
    }
}

// ------------------------------------------------------------ hover buttons

thread_local! {
    static HOVER: Cell<HWND> = const { Cell::new(std::ptr::null_mut()) };
}

pub fn hovered(h: HWND) -> bool {
    HOVER.with(Cell::get) == h
}

/// Hand cursor + hover redraw for an owner-drawn button.
pub fn add_hover(h: HWND) {
    unsafe {
        SetWindowSubclass(h, Some(hover_proc), 1, 0);
    }
}

unsafe extern "system" fn hover_proc(h: HWND, msg: u32, w: WPARAM, l: LPARAM, _id: usize, _data: usize) -> LRESULT {
    const WM_ERASEBKGND: u32 = 0x14;
    const WM_SETCURSOR: u32 = 0x20;
    const WM_MOUSEMOVE: u32 = 0x200;
    const WM_MOUSELEAVE: u32 = 0x2A3;
    match msg {
        WM_MOUSEMOVE => {
            if HOVER.with(Cell::get) != h {
                let old = HOVER.with(|c| c.replace(h));
                if !old.is_null() {
                    InvalidateRect(old, null(), 0);
                }
                InvalidateRect(h, null(), 0);
                let mut t = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: 2, // TME_LEAVE
                    hwndTrack: h,
                    dwHoverTime: 0,
                };
                TrackMouseEvent(&mut t);
            }
        }
        WM_MOUSELEAVE => {
            if HOVER.with(Cell::get) == h {
                HOVER.with(|c| c.set(null_mut()));
                InvalidateRect(h, null(), 0);
            }
        }
        WM_SETCURSOR => {
            SetCursor(LoadCursorW(null_mut(), IDC_HAND));
            return 1;
        }
        WM_ERASEBKGND => return 1,
        _ => {}
    }
    DefSubclassProc(h, msg, w, l)
}

/// Owner-draw DRAWITEMSTRUCT.
#[repr(C)]
pub struct DrawItem {
    pub ctl_type: u32,
    pub ctl_id: u32,
    pub item_id: u32,
    pub item_action: u32,
    pub item_state: u32,
    pub hwnd_item: HWND,
    pub hdc: HDC,
    pub rc: RECT,
    pub item_data: usize,
}

pub const ODS_SELECTED: u32 = 1;
pub const ODS_DISABLED: u32 = 4;

/// The big gradient "Optimise now" style button.
///
/// # Safety
/// `d` must come from a WM_DRAWITEM message.
pub unsafe fn draw_primary(d: &DrawItem, bg: Rgb, label: &str, sub: &str, f: &Fonts, glyph: Option<char>, dpi: i32) {
    let rc = d.rc;
    fill(d.hdc, &rc, bg);
    let disabled = d.item_state & ODS_DISABLED != 0;
    let pressed = d.item_state & ODS_SELECTED != 0;
    let hover = hovered(d.hwnd_item);
    let g = Gfx::new(d.hdc);
    let radius = (12 * dpi / 96) as f32;
    if disabled {
        g.fill_round(&rc, radius, DISABLED, None);
    } else if pressed {
        g.fill_round(&rc, radius, ACCENT_DOWN, None);
    } else if hover {
        g.fill_round_gradient(&rc, radius, ACCENT_HOVER, (29, 78, 216));
    } else {
        g.fill_round_gradient(&rc, radius, ACCENT, BLUE);
    }
    drop(g);
    let fg = if disabled { MUTED } else { WHITE };
    let s = |v: i32| v * dpi / 96;
    let h = rc.bottom - rc.top;
    let has_sub = !sub.is_empty() && h >= s(56);
    let label_font = if has_sub { f.section } else { f.bold };
    let (top, bottom) = if has_sub { (rc.top + s(6), rc.top + h * 3 / 5) } else { (rc.top, rc.bottom) };
    // Centre glyph + label together as one line.
    let glyph_s = glyph.map(|g| g.to_string()).unwrap_or_default();
    let gw = if glyph.is_some() { text_width(d.hdc, &glyph_s, f.icons) + s(10) } else { 0 };
    let lw = text_width(d.hdc, label, label_font);
    let x0 = ((rc.left + rc.right) / 2 - (gw + lw) / 2).max(rc.left + s(8));
    if glyph.is_some() {
        text(d.hdc, &glyph_s, rect(x0, top, x0 + gw, bottom), f.icons, fg, DT_SINGLELINE | DT_VCENTER);
    }
    text(d.hdc, label, rect(x0 + gw, top, rc.right - s(8), bottom), label_font, fg, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
    if has_sub {
        text(d.hdc, sub, rect(rc.left, bottom - s(2), rc.right, rc.bottom - s(6)), f.small, if disabled { MUTED } else { (204, 251, 241) }, DT_SINGLELINE | DT_CENTER);
    }
}

/// Width in pixels of `s` drawn with `font`.
///
/// # Safety
/// `hdc` must be a valid device context.
pub unsafe fn text_width(hdc: HDC, s: &str, font: HFONT) -> i32 {
    SelectObject(hdc, font);
    let w: Vec<u16> = s.encode_utf16().collect();
    let mut sz = SIZE { cx: 0, cy: 0 };
    GetTextExtentPoint32W(hdc, w.as_ptr(), w.len() as i32, &mut sz);
    sz.cx
}
