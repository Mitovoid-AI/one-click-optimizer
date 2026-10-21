//! Setup screen (shown when the exe's name contains "Setup", or with
//! `--install`) and the uninstall flow that Settings > Apps runs.

use crate::install::{self, Layout, Options, VERSION};
use crate::paint::*;
use crate::util::wide;
use crate::{icon, APP_NAME};
use std::cell::{Cell, RefCell};
use std::ptr::{null, null_mut};
use std::sync::Mutex;
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, EndPaint, InvalidateRect,
    SelectObject, HDC, PAINTSTRUCT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_SYSTEM_AWARE};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, DrawIconEx, GetClientRect, GetMessageW,
    GetSystemMetrics, IsDialogMessageW, LoadCursorW, MessageBoxW, MoveWindow, PostMessageW, PostQuitMessage,
    RegisterClassExW, SetForegroundWindow, ShowWindow, TranslateMessage, HICON, IDC_ARROW, MSG, WNDCLASSEXW,
};

const WS_CAPTION_SYSMENU_MIN: u32 = 0x00C0_0000 | 0x0008_0000 | 0x0002_0000;
const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_TABSTOP: u32 = 0x0001_0000;
const BS_OWNERDRAW: u32 = 0xB;
const WM_DESTROY: u32 = 0x02;
const WM_PAINT: u32 = 0x0F;
const WM_ERASEBKGND: u32 = 0x14;
const WM_DRAWITEM: u32 = 0x2B;
const WM_COMMAND: u32 = 0x111;
const WM_APP_INSTALLED: u32 = 0x8000 + 1;
const SW_SHOW: i32 = 5;
const SW_HIDE: i32 = 0;
const MB_YESNO: u32 = 4;
const MB_ICONQUESTION: u32 = 0x20;
const MB_ICONINFORMATION: u32 = 0x40;
const MB_ICONERROR: u32 = 0x10;
const IDYES: i32 = 6;
const SRCCOPY: u32 = 0x00CC_0020;

// Not 1/2: those are IDOK/IDCANCEL, which Enter/Esc send to the window.
const ID_OPT_DESKTOP: u16 = 101;
const ID_OPT_TRAY: u16 = 102;
const ID_INSTALL: u16 = 110;
const ID_SECONDARY: u16 = 111;

const CLASS: &str = "OneClickOptimizerSetup";
const WIN_W: i32 = 580;
const WIN_H: i32 = 680;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Choose,
    Working,
    Done,
    Failed,
}

#[derive(Clone, Copy)]
struct S {
    hwnd: HWND,
    desktop: HWND,
    tray: HWND,
    install: HWND,
    secondary: HWND,
    dpi: i32,
    f: Fonts,
    logo: HICON,
}

thread_local! {
    static ST: Cell<Option<S>> = const { Cell::new(None) };
    static PAGE: Cell<Page> = const { Cell::new(Page::Choose) };
    static OPTS: Cell<(bool, bool)> = const { Cell::new((true, true)) };
    static ERROR: RefCell<String> = const { RefCell::new(String::new()) };
    static RUN_PORTABLE: Cell<bool> = const { Cell::new(false) };
}

static RESULT: Mutex<Option<Result<(), String>>> = Mutex::new(None);

/// The downloaded file is named OneClickOptimizer-Setup.exe: open the installer.
pub fn should_show_setup() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().to_lowercase().contains("setup")))
        .unwrap_or(false)
}

fn st() -> Option<S> {
    ST.with(Cell::get)
}

fn sc(s: &S, v: i32) -> i32 {
    v * s.dpi / 96
}

pub fn run() {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_SYSTEM_AWARE);
        startup();
        let hinst = GetModuleHandleW(null());
        let class = wide(CLASS);
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: Some(proc_),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: hinst,
            hIcon: icon::app_icon_sized(true),
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hbrBackground: null_mut(),
            lpszMenuName: null(),
            lpszClassName: class.as_ptr(),
            hIconSm: icon::app_icon_sized(false),
        };
        RegisterClassExW(&wc);
        let dpi = GetDpiForSystem() as i32;
        let (w, h) = (WIN_W * dpi / 96, WIN_H * dpi / 96);
        let offscreen = std::env::var_os("OCO_OFFSCREEN").is_some();
        let (x, y) = if offscreen { (-10_000, -10_000) } else { ((GetSystemMetrics(0) - w) / 2, (GetSystemMetrics(1) - h) / 2) };
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide(&format!("{APP_NAME} Setup")).as_ptr(),
            WS_CAPTION_SYSMENU_MIN,
            x,
            y,
            w,
            h,
            null_mut(),
            null_mut(),
            hinst,
            null(),
        );
        if hwnd.is_null() {
            return;
        }
        let btn = |id: u16| {
            let b = CreateWindowExW(0, wide("BUTTON").as_ptr(), wide("").as_ptr(), WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW, 0, 0, 10, 10, hwnd, id as usize as _, hinst, null());
            add_hover(b);
            b
        };
        let s = S {
            hwnd,
            desktop: btn(ID_OPT_DESKTOP),
            tray: btn(ID_OPT_TRAY),
            install: btn(ID_INSTALL),
            secondary: btn(ID_SECONDARY),
            dpi,
            f: Fonts::new(dpi),
            logo: icon::app_icon(64 * dpi / 96),
        };
        ST.with(|c| c.set(Some(s)));
        layout(&s);
        if offscreen {
            ShowWindow(hwnd, 4); // SW_SHOWNOACTIVATE: tests never take keyboard focus
        } else {
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
        }
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if IsDialogMessageW(hwnd, &msg) == 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
    if RUN_PORTABLE.with(Cell::get) {
        crate::ui::run();
    }
}

unsafe fn layout(s: &S) {
    let mut rc = rect(0, 0, 0, 0);
    GetClientRect(s.hwnd, &mut rc);
    let (w, h) = (rc.right, rc.bottom);
    let choose = matches!(PAGE.with(Cell::get), Page::Choose | Page::Working | Page::Failed);
    let opt = |y: i32| rect(sc(s, 28), sc(s, y), w - sc(s, 28), sc(s, y + 66));
    let put = |hw: HWND, r: RECT, vis: bool| {
        MoveWindow(hw, r.left, r.top, r.right - r.left, r.bottom - r.top, 1);
        ShowWindow(hw, if vis { SW_SHOW } else { SW_HIDE });
    };
    put(s.desktop, opt(236), choose);
    put(s.tray, opt(312), choose);
    put(s.install, rect(sc(s, 28), h - sc(s, 128), w - sc(s, 28), h - sc(s, 68)), true);
    put(s.secondary, rect(sc(s, 28), h - sc(s, 60), w - sc(s, 28), h - sc(s, 22)), true);
}

unsafe fn paint(s: &S, hdc: HDC, w: i32, h: i32) {
    let l = Layout::standard();
    fill(hdc, &rect(0, 0, w, h), CARD);
    // Header band: the same dark slate as the app's sidebar.
    let band = rect(0, 0, w, sc(s, 176));
    fill(hdc, &band, SIDEBAR);
    let icon_px = sc(s, 64);
    DrawIconEx(hdc, sc(s, 30), sc(s, 38), s.logo, icon_px, icon_px, 0, null_mut(), 3);
    text(hdc, APP_NAME, rect(sc(s, 112), sc(s, 40), w - sc(s, 16), sc(s, 82)), s.f.title, WHITE, DT_SINGLELINE | DT_VCENTER);
    text(hdc, &format!("Faster PC in one click  \u{00B7}  version {VERSION}"), rect(sc(s, 112), sc(s, 84), w - sc(s, 16), sc(s, 108)), s.f.body, SIDEBAR_TEXT, DT_SINGLELINE);
    text(hdc, "Installs just for you: no admin needed, about 1 MB.", rect(sc(s, 30), sc(s, 128), w - sc(s, 16), sc(s, 156)), s.f.small, SIDEBAR_MUTED, DT_SINGLELINE);

    match PAGE.with(Cell::get) {
        Page::Choose | Page::Working | Page::Failed => {
            text(hdc, "Options", rect(sc(s, 30), sc(s, 196), w, sc(s, 226)), s.f.section, INK, DT_SINGLELINE | DT_VCENTER);
            let path = format!("Installs to {}", l.dir.display());
            text(hdc, &path, rect(sc(s, 30), sc(s, 392), w - sc(s, 28), sc(s, 414)), s.f.small, MUTED, DT_SINGLELINE | DT_END_ELLIPSIS);
            let mut y = sc(s, 418);
            if let Some(v) = install::installed_version(&l) {
                let msg = if v == VERSION {
                    format!("Version {v} is already installed. Installing again repairs it.")
                } else {
                    format!("Version {v} is installed and will be updated to {VERSION}.")
                };
                text(hdc, &msg, rect(sc(s, 30), y, w - sc(s, 28), y + sc(s, 22)), s.f.small_bold, ACCENT_DOWN, DT_SINGLELINE | DT_END_ELLIPSIS);
                y += sc(s, 22);
                text(hdc, "OneClick Optimizer will be closed while it updates.", rect(sc(s, 30), y, w - sc(s, 28), y + sc(s, 22)), s.f.small, MUTED, DT_SINGLELINE);
                y += sc(s, 22);
            }
            if PAGE.with(Cell::get) == Page::Failed {
                let err = ERROR.with(|e| e.borrow().clone());
                text(hdc, &format!("Setup didn't finish: {err}"), rect(sc(s, 30), y + sc(s, 6), w - sc(s, 28), h - sc(s, 134)), s.f.bold, BAD, DT_WORDBREAK);
            }
        }
        Page::Done => {
            let (desktop, tray) = OPTS.with(Cell::get);
            let head = rect(sc(s, 30), sc(s, 206), w - sc(s, 28), sc(s, 240));
            text(hdc, "\u{E73E}", rect(head.left, head.top, head.left + sc(s, 30), head.bottom), s.f.icons, GOOD, DT_SINGLELINE | DT_VCENTER);
            text(hdc, "OneClick Optimizer is installed", rect(head.left + sc(s, 34), head.top, head.right, head.bottom), s.f.section, INK, DT_SINGLELINE | DT_VCENTER);
            text(hdc, "Where to find it", rect(sc(s, 30), sc(s, 258), w - sc(s, 28), sc(s, 282)), s.f.small_bold, MUTED, DT_SINGLELINE | DT_VCENTER);
            let mut lines = vec![("Start menu", "search for \u{201C}OneClick Optimizer\u{201D}".to_string())];
            if desktop {
                lines.push(("Desktop", "the OneClick Optimizer icon".to_string()));
            }
            if tray {
                lines.push(("Tray", "the icon next to the clock, starts with Windows".to_string()));
            }
            lines.push(("To remove it", "Settings \u{2192} Apps \u{2192} Installed apps".to_string()));
            for (i, (place, line)) in lines.iter().enumerate() {
                let y = sc(s, 286) + i as i32 * sc(s, 30);
                text(hdc, place, rect(sc(s, 30), y, sc(s, 150), y + sc(s, 28)), s.f.bold, INK, DT_SINGLELINE | DT_VCENTER);
                text(hdc, line, rect(sc(s, 150), y, w - sc(s, 28), y + sc(s, 28)), s.f.body, MUTED, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
            }
        }
    }
}

unsafe fn draw_item(s: &S, d: &DrawItem) {
    let page = PAGE.with(Cell::get);
    match d.ctl_id as u16 {
        id @ (ID_OPT_DESKTOP | ID_OPT_TRAY) => {
            let (desktop, tray) = OPTS.with(Cell::get);
            let on = if id == ID_OPT_DESKTOP { desktop } else { tray };
            let (title, sub) = if id == ID_OPT_DESKTOP {
                ("Desktop shortcut", "Put OneClick Optimizer on your desktop.")
            } else {
                ("Watch memory from the tray", "Starts quietly with Windows. Click the tray icon for one-click Optimise.")
            };
            fill(d.hdc, &d.rc, CARD);
            let g = Gfx::new(d.hdc);
            g.fill_round(&d.rc, sc(s, 12) as f32, if hovered(d.hwnd_item) { BG } else { CARD }, Some(BORDER));
            drop(g);
            let sx = d.rc.right - sc(s, 18) - sc(s, 40);
            draw_switch(d.hdc, &rect(sx, d.rc.top, d.rc.right - sc(s, 18), d.rc.bottom), on, DISABLED, s.dpi);
            text(d.hdc, title, rect(d.rc.left + sc(s, 18), d.rc.top + sc(s, 10), sx - sc(s, 10), d.rc.top + sc(s, 34)), s.f.bold, INK, DT_SINGLELINE);
            text(d.hdc, sub, rect(d.rc.left + sc(s, 18), d.rc.top + sc(s, 34), sx - sc(s, 10), d.rc.bottom - sc(s, 6)), s.f.small, MUTED, DT_SINGLELINE | DT_END_ELLIPSIS);
        }
        ID_INSTALL => {
            let installed = install::installed_version(&Layout::standard());
            let label = match page {
                Page::Choose | Page::Failed => match installed.as_deref() {
                    Some(v) if v == VERSION => "Repair install".to_string(),
                    Some(_) => format!("Update to {VERSION}"),
                    None => "Install".to_string(),
                },
                Page::Working => "Installing\u{2026}".to_string(),
                Page::Done => "Open OneClick Optimizer".to_string(),
            };
            draw_primary(d, CARD, &label, "", &s.f, None, s.dpi);
        }
        ID_SECONDARY => {
            fill(d.hdc, &d.rc, CARD);
            let label = if page == Page::Done { "Close" } else { "Just run it without installing" };
            let fg = if hovered(d.hwnd_item) { ACCENT_DOWN } else { ACCENT };
            text(d.hdc, label, d.rc, s.f.bold, fg, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
        _ => {}
    }
}

unsafe fn start_install(s: &S) {
    PAGE.with(|p| p.set(Page::Working));
    EnableWindow(s.install, 0);
    EnableWindow(s.secondary, 0);
    InvalidateRect(s.hwnd, null(), 0);
    let (desktop, tray) = OPTS.with(Cell::get);
    let target = s.hwnd as usize;
    std::thread::spawn(move || {
        let l = Layout::standard();
        let result = std::env::current_exe()
            .map_err(|e| e.to_string())
            .and_then(|src| install::install(&src, &l, &Options { desktop_shortcut: desktop, tray_at_startup: tray }));
        if result.is_ok() && tray && l.live {
            // Show the tray icon right away, not only after the next sign-in.
            let _ = std::process::Command::new(l.exe()).arg("agent").spawn();
        }
        *RESULT.lock().unwrap_or_else(|e| e.into_inner()) = Some(result);
        unsafe { PostMessageW(target as HWND, WM_APP_INSTALLED, 0, 0) };
    });
}

unsafe fn on_paint(s: &S) {
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let hdc = BeginPaint(s.hwnd, &mut ps);
    let mut rc = rect(0, 0, 0, 0);
    GetClientRect(s.hwnd, &mut rc);
    let (w, h) = (rc.right.max(1), rc.bottom.max(1));
    let mdc = CreateCompatibleDC(hdc);
    let bmp = CreateCompatibleBitmap(hdc, w, h);
    let old = SelectObject(mdc, bmp);
    paint(s, mdc, w, h);
    BitBlt(hdc, 0, 0, w, h, mdc, 0, 0, SRCCOPY);
    SelectObject(mdc, old);
    DeleteObject(bmp);
    DeleteDC(mdc);
    EndPaint(s.hwnd, &ps);
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            match st() {
                Some(s) => on_paint(&s),
                None => return DefWindowProcW(hwnd, msg, wparam, lparam),
            }
            0
        }
        WM_DRAWITEM => {
            if let Some(s) = st() {
                draw_item(&s, &*(lparam as *const DrawItem));
            }
            1
        }
        WM_COMMAND => {
            let Some(s) = st() else { return 0 };
            let page = PAGE.with(Cell::get);
            match (wparam & 0xFFFF) as u16 {
                ID_OPT_DESKTOP => {
                    OPTS.with(|o| o.set((!o.get().0, o.get().1)));
                    InvalidateRect(s.desktop, null(), 0);
                }
                ID_OPT_TRAY => {
                    OPTS.with(|o| o.set((o.get().0, !o.get().1)));
                    InvalidateRect(s.tray, null(), 0);
                }
                ID_INSTALL => match page {
                    Page::Choose | Page::Failed => start_install(&s),
                    Page::Done => {
                        let _ = std::process::Command::new(Layout::standard().exe()).spawn();
                        DestroyWindow(hwnd);
                    }
                    Page::Working => {}
                },
                ID_SECONDARY => {
                    if page != Page::Done {
                        RUN_PORTABLE.with(|r| r.set(true));
                    }
                    DestroyWindow(hwnd);
                }
                _ => {}
            }
            0
        }
        WM_APP_INSTALLED => {
            if let Some(s) = st() {
                let result = RESULT.lock().unwrap_or_else(|e| e.into_inner()).take().unwrap_or(Err("unknown error".into()));
                match result {
                    Ok(()) => PAGE.with(|p| p.set(Page::Done)),
                    Err(e) => {
                        ERROR.with(|x| *x.borrow_mut() = e);
                        PAGE.with(|p| p.set(Page::Failed));
                    }
                }
                EnableWindow(s.install, 1);
                EnableWindow(s.secondary, 1);
                layout(&s);
                InvalidateRect(hwnd, null(), 0);
                InvalidateRect(s.install, null(), 0);
                InvalidateRect(s.secondary, null(), 0);
            }
            0
        }
        WM_DESTROY => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// `OneClickOptimizer-Setup.exe --update`: started by the app's "Update now".
/// Installs over the current version keeping the user's choices, then
/// restarts the app (and the tray watch if it was running).
pub fn update_mode() -> i32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::FindWindowW;
    let title = wide(APP_NAME);
    unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_SYSTEM_AWARE) };
    let l = Layout::standard();
    let tray_running = unsafe { !FindWindowW(wide(crate::agent::TRAY_CLASS).as_ptr(), null()).is_null() };
    // Let the app that started us finish exiting.
    std::thread::sleep(std::time::Duration::from_millis(800));
    let opts = Options {
        desktop_shortcut: l.desktop_lnk.exists(),
        tray_at_startup: l.live && crate::sys::startup::agent_autostart(),
    };
    let result = std::env::current_exe().map_err(|e| e.to_string()).and_then(|src| install::install(&src, &l, &opts));
    let quiet = std::env::var_os("OCO_OFFSCREEN").is_some();
    match result {
        Ok(()) => {
            if l.live {
                if tray_running || opts.tray_at_startup {
                    let _ = std::process::Command::new(l.exe()).arg("agent").spawn();
                }
                let _ = std::process::Command::new(l.exe()).spawn();
            }
            if !quiet {
                let msg = wide(&format!("OneClick Optimizer was updated to version {VERSION}."));
                unsafe { MessageBoxW(null_mut(), msg.as_ptr(), title.as_ptr(), MB_ICONINFORMATION) };
            }
            0
        }
        Err(e) => {
            if !quiet {
                let msg = wide(&format!("The update didn't finish: {e}\n\nYour current version still works."));
                unsafe { MessageBoxW(null_mut(), msg.as_ptr(), title.as_ptr(), MB_ICONERROR) };
            }
            2
        }
    }
}

/// `OneClickOptimizer.exe --uninstall [--quiet]` (run by Settings > Apps).
pub fn uninstall_flow(quiet: bool) -> i32 {
    let title = wide(APP_NAME);
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_SYSTEM_AWARE);
        if !quiet {
            let q = wide(
                "Remove OneClick Optimizer from this PC?\n\n\
                 This removes the app, its shortcuts, its tray icon and its history.\n\
                 Changes it made to your PC stay as they are (undo them first in History & undo if you want them back).",
            );
            if MessageBoxW(null_mut(), q.as_ptr(), title.as_ptr(), MB_YESNO | MB_ICONQUESTION) != IDYES {
                return 1;
            }
        }
        match install::uninstall(&Layout::standard(), true) {
            Ok(()) => {
                if !quiet {
                    MessageBoxW(null_mut(), wide("OneClick Optimizer was removed.").as_ptr(), title.as_ptr(), MB_ICONINFORMATION);
                }
                0
            }
            Err(e) => {
                if !quiet {
                    MessageBoxW(null_mut(), wide(&format!("Uninstall didn't finish: {e}")).as_ptr(), title.as_ptr(), MB_ICONERROR);
                }
                2
            }
        }
    }
}
