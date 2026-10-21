//! The app's own notification windows, drawn like the rest of the app:
//! - `show_toast`: a small card above the tray (memory warnings, idle dev
//!   tools, a new version, Autopilot results) with one action button. It
//!   hides itself after a few seconds unless the mouse is on it, and waits
//!   while you present or play full-screen (see `notifications_allowed`).
//! - `update_window`: "Version x is ready": what's new, then Update now /
//!   Later / Skip this version, with a progress bar while it downloads.

use crate::paint::*;
use crate::update::{self, Release};
use crate::util::{fmt_bytes, wide};
use crate::{icon, APP_NAME};
use std::cell::{Cell, RefCell};
use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, DrawTextW, EndPaint,
    InvalidateRect, SelectObject, HDC, PAINTSTRUCT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AnimateWindow, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyWindow, DispatchMessageW, DrawIconEx,
    GetClientRect, GetCursorPos, GetMessageW, GetWindowRect, IsWindow, KillTimer, LoadCursorW, MoveWindow,
    PostMessageW, PostQuitMessage, RegisterClassExW, SetForegroundWindow, SetTimer, ShowWindow, SystemParametersInfoW,
    TranslateMessage, HICON, IDC_ARROW, MSG, WNDCLASSEXW,
};

const WM_DESTROY: u32 = 0x02;
const WM_PAINT: u32 = 0x0F;
const WM_CLOSE: u32 = 0x10;
const WM_ERASEBKGND: u32 = 0x14;
const WM_DRAWITEM: u32 = 0x2B;
const WM_KEYDOWN: u32 = 0x100;
const WM_COMMAND: u32 = 0x111;
const WM_TIMER: u32 = 0x113;
const WM_APP: u32 = 0x8000;
/// Sent to the toast's owner when its action button is clicked (wparam = tag).
pub const WM_TOAST_ACTION: u32 = WM_APP + 40;
const WM_UPD_PROGRESS: u32 = WM_APP + 41;
const WM_UPD_DONE: u32 = WM_APP + 42;

const WS_POPUP: u32 = 0x8000_0000;
const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_CAPTION: u32 = 0x00C0_0000;
const WS_SYSMENU: u32 = 0x0008_0000;
const WS_TABSTOP: u32 = 0x0001_0000;
const WS_EX_TOPMOST: u32 = 0x8;
const WS_EX_TOOLWINDOW: u32 = 0x80;
const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
const WS_EX_DLGMODALFRAME: u32 = 0x1;
const BS_OWNERDRAW: u32 = 0xB;
const CS_DROPSHADOW: u32 = 0x0002_0000;
const SW_HIDE: i32 = 0;
const SW_SHOW: i32 = 5;
const SW_SHOWNOACTIVATE: i32 = 4;
const SPI_GETWORKAREA: u32 = 0x30;
const SRCCOPY: u32 = 0x00CC_0020;
const AW_BLEND: u32 = 0x0008_0000;
const DT_CALCRECT: u32 = 0x400;
const VK_RETURN: usize = 0x0D;
const VK_ESCAPE: usize = 0x1B;

const TOAST_CLASS: &str = "OneClickOptimizerToast";
const UPDATE_CLASS: &str = "OneClickOptimizerUpdate";

#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(hwnd: HWND, attr: u32, value: *const std::ffi::c_void, size: u32) -> i32;
}

fn offscreen() -> bool {
    std::env::var_os("OCO_OFFSCREEN").is_some()
}

fn sc(dpi: i32, v: i32) -> i32 {
    v * dpi / 96
}

thread_local! {
    static FONTS: Cell<Option<Fonts>> = const { Cell::new(None) };
}

fn fonts(dpi: i32) -> Fonts {
    FONTS.with(|f| {
        if let Some(x) = f.get() {
            return x;
        }
        let x = unsafe { Fonts::new(dpi) };
        f.set(Some(x));
        x
    })
}

unsafe fn register(class: &str, proc_: unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT, style: u32) {
    let name = wide(class);
    let wc = WNDCLASSEXW {
        cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
        style,
        lpfnWndProc: Some(proc_),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: GetModuleHandleW(null()),
        hIcon: icon::app_icon_sized(true),
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: null_mut(),
        lpszMenuName: null(),
        lpszClassName: name.as_ptr(),
        hIconSm: icon::app_icon_sized(false),
    };
    RegisterClassExW(&wc); // fails harmlessly when already registered
}

unsafe fn button(parent: HWND, id: u16) -> HWND {
    let b = CreateWindowExW(0, wide("BUTTON").as_ptr(), wide("").as_ptr(), WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW, 0, 0, 10, 10, parent, id as usize as _, GetModuleHandleW(null()), null());
    add_hover(b);
    b
}

unsafe fn place(h: HWND, r: RECT, visible: bool) {
    if visible {
        MoveWindow(h, r.left, r.top, r.right - r.left, r.bottom - r.top, 1);
    }
    ShowWindow(h, if visible { SW_SHOW } else { SW_HIDE });
}

/// Height of `s` wrapped to `width` pixels.
unsafe fn text_height(hdc: HDC, s: &str, font: windows_sys::Win32::Graphics::Gdi::HFONT, width: i32) -> i32 {
    SelectObject(hdc, font);
    let w = wide(s);
    let mut r = rect(0, 0, width, 0);
    DrawTextW(hdc, w.as_ptr(), -1, &mut r, DT_WORDBREAK | DT_CALCRECT | DT_NOPREFIX);
    r.bottom
}

/// Double-buffered paint.
unsafe fn buffered(hwnd: HWND, draw: impl FnOnce(HDC, i32, i32)) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut rc = rect(0, 0, 0, 0);
    GetClientRect(hwnd, &mut rc);
    let (w, h) = (rc.right.max(1), rc.bottom.max(1));
    let mem = CreateCompatibleDC(hdc);
    let bmp = CreateCompatibleBitmap(hdc, w, h);
    let old = SelectObject(mem, bmp);
    draw(mem, w, h);
    BitBlt(hdc, 0, 0, w, h, mem, 0, 0, SRCCOPY);
    SelectObject(mem, old);
    DeleteObject(bmp);
    DeleteDC(mem);
    EndPaint(hwnd, &ps);
}

fn work_area() -> RECT {
    let mut work = rect(0, 0, 1280, 720);
    unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut RECT as *mut _, 0) };
    work
}

/// False while you present, play full-screen or have quiet hours on.
pub fn notifications_allowed() -> bool {
    crate::autopilot::user_not_busy()
}

// ==================================================================== toast

pub struct Toast {
    pub title: String,
    pub body: String,
    /// Label of the one action button (None = just the close button).
    pub action: Option<String>,
    /// Passed back to the owner in WM_TOAST_ACTION.
    pub tag: usize,
}

struct ToastWin {
    hwnd: HWND,
    owner: HWND,
    close: HWND,
    action: HWND,
    toast: Toast,
    dpi: i32,
    logo: HICON,
    shown_ms: u32,
}

thread_local! {
    static TOAST: RefCell<Option<ToastWin>> = const { RefCell::new(None) };
}

const ID_T_CLOSE: u16 = 101;
const ID_T_ACTION: u16 = 102;
const TIMER_T: usize = 1;
const TOAST_W: i32 = 372;
const TOAST_SECS: u32 = 10;

/// Shows a toast above the tray (replacing one already showing).
pub fn show_toast(owner: HWND, t: Toast) {
    close_toast();
    unsafe {
        register(TOAST_CLASS, toast_proc, CS_DROPSHADOW);
        let dpi = GetDpiForSystem() as i32;
        let s = |v: i32| sc(dpi, v);
        let f = fonts(dpi);
        let w = s(TOAST_W);
        // Height from the wrapped body text.
        let dc = CreateCompatibleDC(null_mut());
        let body_h = text_height(dc, &t.body, f.small, w - s(32)).min(s(80));
        let action_w = t.action.as_deref().map_or(0, |label| text_width(dc, label, f.bold) + s(36));
        DeleteDC(dc);
        let actions = if t.action.is_some() { s(14) + s(34) } else { 0 };
        let h = s(16) + s(24) + s(6) + body_h + actions + s(16);
        let work = work_area();
        let (x, y) = if offscreen() { (-10_000, -10_000) } else { (work.right - w - s(12), work.bottom - h - s(12)) };
        let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE, wide(TOAST_CLASS).as_ptr(), wide(APP_NAME).as_ptr(), WS_POPUP, x, y, w, h, null_mut(), null_mut(), GetModuleHandleW(null()), null());
        if hwnd.is_null() {
            return;
        }
        let round: u32 = 2;
        DwmSetWindowAttribute(hwnd, 33, &round as *const u32 as *const _, 4);
        let close = button(hwnd, ID_T_CLOSE);
        let action = button(hwnd, ID_T_ACTION);
        MoveWindow(close, w - s(42), s(10), s(30), s(30), 1);
        if t.action.is_some() {
            MoveWindow(action, w - s(16) - action_w, h - s(16) - s(34), action_w, s(34), 1);
        } else {
            ShowWindow(action, SW_HIDE);
        }
        TOAST.with(|c| *c.borrow_mut() = Some(ToastWin { hwnd, owner, close, action, toast: t, dpi, logo: icon::app_icon(s(20)), shown_ms: 0 }));
        SetTimer(hwnd, TIMER_T, 500, None);
        if offscreen() {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        } else if AnimateWindow(hwnd, 160, AW_BLEND) == 0 {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }
}


pub fn close_toast() {
    let h = TOAST.with(|c| c.borrow().as_ref().map(|t| t.hwnd));
    if let Some(h) = h {
        unsafe { DestroyWindow(h) };
    }
}

/// The toast window currently showing, if any (tests and screenshots).
pub fn toast_hwnd() -> Option<HWND> {
    TOAST.with(|c| c.borrow().as_ref().map(|t| t.hwnd))
}

unsafe fn toast_paint(t: &ToastWin, hdc: HDC, w: i32, h: i32) {
    let s = |v: i32| sc(t.dpi, v);
    let f = fonts(t.dpi);
    fill(hdc, &rect(0, 0, w, h), BORDER);
    fill(hdc, &rect(1, 1, w - 1, h - 1), CARD);
    DrawIconEx(hdc, s(16), s(18), t.logo, s(20), s(20), 0, null_mut(), 3);
    text(hdc, &t.toast.title, rect(s(46), s(16), w - s(48), s(40)), f.bold, INK, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
    text(hdc, &t.toast.body, rect(s(16), s(46), w - s(16), h - s(14)), f.small, MUTED, DT_WORDBREAK | DT_END_ELLIPSIS);
}

unsafe extern "system" fn toast_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            TOAST.with(|c| {
                if let Some(t) = c.borrow().as_ref().filter(|t| t.hwnd == hwnd) {
                    buffered(hwnd, |hdc, w, h| toast_paint(t, hdc, w, h));
                }
            });
            0
        }
        WM_DRAWITEM => {
            let d = &*(lparam as *const DrawItem);
            let info = TOAST.with(|c| c.borrow().as_ref().map(|t| (t.dpi, t.toast.action.clone().unwrap_or_default())));
            if let Some((dpi, label)) = info {
                let f = fonts(dpi);
                match d.ctl_id as u16 {
                    ID_T_CLOSE => {
                        fill(d.hdc, &d.rc, CARD);
                        if hovered(d.hwnd_item) {
                            Gfx::new(d.hdc).fill_round(&d.rc, sc(dpi, 6) as f32, BG, None);
                        }
                        text(d.hdc, "\u{E711}", d.rc, f.icons, MUTED, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                    }
                    _ => draw_primary(d, CARD, &label, "", &f, None, dpi),
                }
            }
            1
        }
        WM_COMMAND => {
            let id = (wparam & 0xFFFF) as u16;
            let (owner, tag) = TOAST.with(|c| c.borrow().as_ref().map_or((null_mut(), 0), |t| (t.owner, t.toast.tag)));
            if id == ID_T_ACTION && !owner.is_null() {
                PostMessageW(owner, WM_TOAST_ACTION, tag, 0);
            }
            DestroyWindow(hwnd);
            0
        }
        WM_TIMER => {
            // Count down only while the mouse is not on the toast.
            let mut pt = POINT { x: 0, y: 0 };
            let mut r = rect(0, 0, 0, 0);
            GetCursorPos(&mut pt);
            GetWindowRect(hwnd, &mut r);
            let over = pt.x >= r.left && pt.x < r.right && pt.y >= r.top && pt.y < r.bottom;
            let expired = TOAST.with(|c| {
                let mut b = c.borrow_mut();
                let Some(t) = b.as_mut() else { return false };
                t.shown_ms = if over { 0 } else { t.shown_ms + 500 };
                t.shown_ms >= TOAST_SECS * 1000 && !offscreen()
            });
            if expired {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_DESTROY => {
            KillTimer(hwnd, TIMER_T);
            TOAST.with(|c| {
                let mut b = c.borrow_mut();
                if b.as_ref().is_some_and(|t| t.hwnd == hwnd) {
                    if let Some(t) = b.take() {
                        DestroyIcon(t.logo);
                        let _ = (t.close, t.action);
                    }
                }
            });
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// ============================================================ update window

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateOutcome {
    /// The new version is being installed: the caller should exit now.
    Installing,
    Skipped,
    Later,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UState {
    Ask,
    Working,
    Failed,
    Done,
}

struct UpdWin {
    hwnd: HWND,
    update: HWND,
    later: HWND,
    skip: HWND,
    release: Release,
    items: Vec<String>,
    dpi: i32,
    logo: HICON,
    state: UState,
    got: u64,
    total: u64,
    error: String,
    outcome: UpdateOutcome,
}

thread_local! {
    static UPD: RefCell<Option<UpdWin>> = const { RefCell::new(None) };
}

const ID_U_UPDATE: u16 = 201;
const ID_U_LATER: u16 = 202;
const ID_U_SKIP: u16 = 203;
const TIMER_U_CLOSE: usize = 2;
const UPD_W: i32 = 500;
const MAX_ITEMS: usize = 6;

/// Geometry shared by layout and paint.
struct ULayout {
    items_top: i32,
    item_tops: Vec<(i32, i32)>,
    footer: i32,
    height: i32,
}

unsafe fn upd_layout(dpi: i32, items: &[String]) -> ULayout {
    let s = |v: i32| sc(dpi, v);
    let f = fonts(dpi);
    let dc = CreateCompatibleDC(null_mut());
    let width = s(UPD_W) - s(48) - s(16);
    let items_top = s(120);
    let mut y = items_top;
    let mut item_tops = Vec::new();
    for it in items.iter().take(MAX_ITEMS) {
        let h = text_height(dc, it, f.body, width).max(s(20));
        item_tops.push((y, h));
        y += h + s(8);
    }
    DeleteDC(dc);
    if items.is_empty() {
        y += s(4);
    }
    let footer = y + s(14);
    ULayout { items_top, item_tops, footer, height: footer + s(76) }
}

unsafe fn upd_place_buttons(u: &UpdWin) {
    let s = |v: i32| sc(u.dpi, v);
    let mut rc = rect(0, 0, 0, 0);
    GetClientRect(u.hwnd, &mut rc);
    let l = upd_layout(u.dpi, &u.items);
    let y = l.footer + s(18);
    let (bh, gap) = (s(40), s(10));
    let w_update = s(150);
    let w_later = s(100);
    let right = rc.right - s(24);
    let ask = u.state == UState::Ask;
    let failed = u.state == UState::Failed;
    place(u.update, rect(right - w_update, y, right, y + bh), ask);
    // After a failure "Later" becomes "Close" in the corner.
    let later = if failed { rect(right - w_later, y, right, y + bh) } else { rect(right - w_update - gap - w_later, y, right - w_update - gap, y + bh) };
    place(u.later, later, ask || failed);
    place(u.skip, rect(s(24), y, s(24) + s(150), y + bh), ask);
}

unsafe fn upd_paint(u: &UpdWin, hdc: HDC, w: i32, h: i32) {
    let s = |v: i32| sc(u.dpi, v);
    let f = fonts(u.dpi);
    fill(hdc, &rect(0, 0, w, h), CARD);
    DrawIconEx(hdc, s(24), s(24), u.logo, s(40), s(40), 0, null_mut(), 3);
    let title = format!("Version {} is ready", u.release.version);
    text(hdc, &title, rect(s(80), s(22), w - s(24), s(50)), f.section, INK, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
    let size = u.release.download_size();
    let mut sub = format!("You have {}", update::current_version());
    if size > 0 {
        sub.push_str(&format!(" \u{00B7} {} download", fmt_bytes(size)));
    }
    sub.push_str(" \u{00B7} keeps your settings and history");
    text(hdc, &sub, rect(s(80), s(50), w - s(24), s(72)), f.small, MUTED, DT_SINGLELINE | DT_END_ELLIPSIS);

    let l = upd_layout(u.dpi, &u.items);
    text(hdc, "What's new", rect(s(24), s(92), w - s(24), s(114)), f.small_bold, MUTED, DT_SINGLELINE | DT_VCENTER);
    if u.items.is_empty() {
        text(hdc, "Improvements and fixes.", rect(s(24), l.items_top, w - s(24), l.items_top + s(22)), f.body, INK, DT_SINGLELINE);
    }
    let gfx = Gfx::new(hdc);
    for &(y, _) in &l.item_tops {
        gfx.dot((s(24) + s(3)) as f32, (y + s(10)) as f32, s(2) as f32, MUTED);
    }
    drop(gfx);
    for (it, &(y, ih)) in u.items.iter().zip(&l.item_tops) {
        text(hdc, it, rect(s(40), y, w - s(24), y + ih), f.body, INK, DT_WORDBREAK);
    }
    if u.items.len() > MAX_ITEMS {
        let y = l.item_tops.last().map_or(l.items_top, |&(y, ih)| y + ih + s(2));
        text(hdc, &format!("and {} more", u.items.len() - MAX_ITEMS), rect(s(40), y, w - s(24), y + s(20)), f.small, MUTED, DT_SINGLELINE);
    }

    // Footer.
    fill(hdc, &rect(0, l.footer, w, l.footer + 1), BORDER);
    fill(hdc, &rect(0, l.footer + 1, w, h), (248, 250, 252));
    let fy = l.footer + s(18);
    match u.state {
        UState::Ask => {}
        UState::Working | UState::Done => {
            let msg = if u.state == UState::Done {
                "Installed. Restarting OneClick Optimizer\u{2026}".to_string()
            } else if u.total > 0 && u.got >= u.total {
                "Checking the download\u{2026}".to_string()
            } else if u.total > 0 && u.got > 0 {
                format!("Downloading {} of {}\u{2026}", fmt_bytes(u.got), fmt_bytes(u.total))
            } else {
                "Downloading\u{2026}".to_string()
            };
            text(hdc, &msg, rect(s(24), fy - s(4), w - s(24), fy + s(18)), f.small_bold, INK, DT_SINGLELINE | DT_VCENTER);
            let bar = rect(s(24), fy + s(26), w - s(24), fy + s(34));
            let frac = if u.state == UState::Done { 1.0 } else if u.total > 0 { (u.got as f64 / u.total as f64).clamp(0.02, 1.0) } else { 0.1 };
            let gfx = Gfx::new(hdc);
            gfx.fill_round(&bar, s(4) as f32, TRACK, None);
            let fillw = ((bar.right - bar.left) as f64 * frac) as i32;
            gfx.fill_round(&rect(bar.left, bar.top, bar.left + fillw.max(s(8)), bar.bottom), s(4) as f32, ACCENT, None);
        }
        UState::Failed => {
            text(hdc, &format!("The update couldn't be installed: {}. Your current version keeps working.", u.error), rect(s(24), fy - s(6), w - s(24) - s(110), h - s(8)), f.small, BAD, DT_WORDBREAK | DT_END_ELLIPSIS);
        }
    }
}

unsafe fn upd_start(hwnd: HWND) {
    let r = UPD.with(|c| {
        let mut b = c.borrow_mut();
        let u = b.as_mut()?;
        u.state = UState::Working;
        u.total = u.release.download_size();
        Some(u.release.clone())
    });
    let Some(r) = r else { return };
    UPD.with(|c| {
        if let Some(u) = c.borrow().as_ref() {
            upd_place_buttons(u);
        }
    });
    InvalidateRect(hwnd, null(), 0);
    let target = hwnd as usize;
    std::thread::spawn(move || {
        let ok = update::apply(&r, &|got, total| unsafe {
            PostMessageW(target as HWND, WM_UPD_PROGRESS, got as usize, total as isize);
        });
        let code = match ok {
            Ok(()) => 1,
            Err(e) => {
                *UPD_ERROR.lock().unwrap_or_else(|x| x.into_inner()) = Some(e);
                0
            }
        };
        unsafe { PostMessageW(target as HWND, WM_UPD_DONE, code, 0) };
    });
}

static UPD_ERROR: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

unsafe extern "system" fn upd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            UPD.with(|c| {
                if let Some(u) = c.borrow().as_ref().filter(|u| u.hwnd == hwnd) {
                    buffered(hwnd, |hdc, w, h| upd_paint(u, hdc, w, h));
                }
            });
            0
        }
        WM_DRAWITEM => {
            let d = &*(lparam as *const DrawItem);
            if let Some((dpi, state)) = UPD.with(|c| c.borrow().as_ref().map(|u| (u.dpi, u.state))) {
                let f = fonts(dpi);
                let bg = (248, 250, 252);
                match d.ctl_id as u16 {
                    ID_U_UPDATE => draw_primary(d, bg, "Update now", "", &f, None, dpi),
                    ID_U_LATER => draw_secondary(d, bg, if state == UState::Failed { "Close" } else { "Later" }, &f, dpi),
                    _ => draw_link(d, bg, "Skip this version", &f, 0),
                }
            }
            1
        }
        WM_COMMAND => {
            match (wparam & 0xFFFF) as u16 {
                ID_U_UPDATE => upd_start(hwnd),
                ID_U_SKIP => {
                    UPD.with(|c| {
                        if let Some(u) = c.borrow_mut().as_mut() {
                            update::skip(&u.release.version);
                            u.outcome = UpdateOutcome::Skipped;
                        }
                    });
                    DestroyWindow(hwnd);
                }
                ID_U_LATER => {
                    DestroyWindow(hwnd);
                }
                _ => {}
            }
            0
        }
        WM_UPD_PROGRESS => {
            UPD.with(|c| {
                if let Some(u) = c.borrow_mut().as_mut() {
                    u.got = wparam as u64;
                    if lparam > 0 {
                        u.total = lparam as u64;
                    }
                }
            });
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_UPD_DONE => {
            let ok = wparam == 1;
            UPD.with(|c| {
                if let Some(u) = c.borrow_mut().as_mut() {
                    if ok {
                        u.state = UState::Done;
                        u.outcome = UpdateOutcome::Installing;
                    } else {
                        u.state = UState::Failed;
                        u.error = UPD_ERROR.lock().unwrap_or_else(|x| x.into_inner()).take().unwrap_or_default().trim_end_matches('.').to_string();
                    }
                    upd_place_buttons(u);
                }
            });
            InvalidateRect(hwnd, null(), 0);
            if ok {
                SetTimer(hwnd, TIMER_U_CLOSE, 900, None);
            }
            0
        }
        WM_TIMER => {
            if wparam == TIMER_U_CLOSE {
                KillTimer(hwnd, TIMER_U_CLOSE);
                DestroyWindow(hwnd);
            }
            0
        }
        WM_CLOSE => {
            // Never interrupt a download half way.
            let working = UPD.with(|c| c.borrow().as_ref().is_some_and(|u| u.state == UState::Working));
            if !working {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_DESTROY => 0,
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Shows the update window and waits for the answer (modal to `parent`).
/// `Installing` means the new version is starting: exit right away.
pub fn update_window(parent: HWND, r: &Release) -> UpdateOutcome {
    unsafe {
        register(UPDATE_CLASS, upd_proc, 0);
        let dpi = GetDpiForSystem() as i32;
        let s = |v: i32| sc(dpi, v);
        let items = update::whats_new_items(&r.notes);
        let l = upd_layout(dpi, &items);
        // Outer size from the client size we want.
        let mut frame = rect(0, 0, s(UPD_W), l.height);
        windows_sys::Win32::UI::WindowsAndMessaging::AdjustWindowRectEx(&mut frame, WS_CAPTION | WS_SYSMENU, 0, WS_EX_DLGMODALFRAME);
        let (w, h) = (frame.right - frame.left, frame.bottom - frame.top);
        let center = if parent.is_null() {
            work_area()
        } else {
            let mut pr = rect(0, 0, 0, 0);
            GetWindowRect(parent, &mut pr);
            pr
        };
        let (x, y) = if offscreen() { (-10_000, -10_000) } else { ((center.left + center.right - w) / 2, ((center.top + center.bottom - h) / 2).max(0)) };
        let hwnd = CreateWindowExW(
            WS_EX_DLGMODALFRAME | if parent.is_null() { WS_EX_TOPMOST } else { 0 },
            wide(UPDATE_CLASS).as_ptr(),
            wide(update::DIALOG_TITLE).as_ptr(),
            WS_POPUP | WS_CAPTION | WS_SYSMENU,
            x,
            y,
            w,
            h,
            parent,
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        );
        if hwnd.is_null() {
            return UpdateOutcome::Later;
        }
        let u = UpdWin {
            hwnd,
            update: button(hwnd, ID_U_UPDATE),
            later: button(hwnd, ID_U_LATER),
            skip: button(hwnd, ID_U_SKIP),
            release: r.clone(),
            items,
            dpi,
            logo: icon::app_icon(s(40)),
            state: UState::Ask,
            got: 0,
            total: 0,
            error: String::new(),
            outcome: UpdateOutcome::Later,
        };
        upd_place_buttons(&u);
        UPD.with(|c| *c.borrow_mut() = Some(u));
        if !parent.is_null() {
            EnableWindow(parent, 0);
        }
        ShowWindow(hwnd, if offscreen() { SW_SHOWNOACTIVATE } else { SW_SHOW });
        if !offscreen() {
            SetForegroundWindow(hwnd);
        }
        let mut msg: MSG = std::mem::zeroed();
        let mut quit = false;
        while IsWindow(hwnd) != 0 {
            if GetMessageW(&mut msg, null_mut(), 0, 0) <= 0 {
                quit = true;
                break;
            }
            if msg.message == WM_KEYDOWN && (msg.hwnd == hwnd || windows_sys::Win32::UI::WindowsAndMessaging::GetParent(msg.hwnd) == hwnd) {
                let state = UPD.with(|c| c.borrow().as_ref().map(|u| u.state));
                match (msg.wParam, state) {
                    (VK_ESCAPE, Some(UState::Ask | UState::Failed)) => {
                        DestroyWindow(hwnd);
                        continue;
                    }
                    (VK_RETURN, Some(UState::Ask)) => {
                        upd_start(hwnd);
                        continue;
                    }
                    _ => {}
                }
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        if !parent.is_null() {
            EnableWindow(parent, 1);
            SetForegroundWindow(parent);
        }
        let outcome = UPD.with(|c| {
            let u = c.borrow_mut().take();
            u.map_or(UpdateOutcome::Later, |u| {
                DestroyIcon(u.logo);
                u.outcome
            })
        });
        if quit {
            PostQuitMessage(msg.wParam as i32);
        }
        outcome
    }
}

/// The update window currently open on this thread (tests and screenshots).
pub fn update_hwnd() -> Option<HWND> {
    UPD.with(|c| c.borrow().as_ref().map(|u| u.hwnd))
}
