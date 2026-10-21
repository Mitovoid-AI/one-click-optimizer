//! Tray watch: an icon next to the clock that tracks memory pressure every
//! 5 seconds, warns only when something matters, and opens a small quick
//! panel on click with one-click Optimise.

use crate::analyze::{self, Category, Level};
use crate::model::{MemInfo, Snapshot};
use crate::optimize::{self, RunSummary, ScanResult};
use crate::paint::*;
use crate::util::{fmt_bytes, wide};
use crate::{actions::Op, icon, report, sys, APP_NAME};
use std::cell::{Cell, RefCell};
use std::ptr::{null, null_mut};
use std::sync::Mutex;
use windows_sys::Win32::Foundation::{GetLastError, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, EndPaint, InvalidateRect,
    SelectObject, HDC, PAINTSTRUCT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_SYSTEM_AWARE};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows_sys::Win32::UI::Shell::{Shell_NotifyIconW, NOTIFYICONDATAW};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyIcon, DestroyMenu, DestroyWindow,
    DispatchMessageW, DrawIconEx, GetClientRect, GetCursorPos, GetMessageW, KillTimer, LoadCursorW, PostMessageW,
    PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SetForegroundWindow, SetTimer, ShowWindow,
    SystemParametersInfoW, TrackPopupMenu, TranslateMessage, HICON, IDC_ARROW, MSG, WNDCLASSEXW,
};

const WM_DESTROY: u32 = 0x02;
const WM_ACTIVATE: u32 = 0x06;
const WM_PAINT: u32 = 0x0F;
const WM_ERASEBKGND: u32 = 0x14;
const WM_DRAWITEM: u32 = 0x2B;
const WM_COMMAND: u32 = 0x111;
const WM_TIMER: u32 = 0x113;
const WM_LBUTTONUP: u32 = 0x202;
const WM_LBUTTONDBLCLK: u32 = 0x203;
const WM_RBUTTONUP: u32 = 0x205;
const WM_CONTEXTMENU: u32 = 0x7B;
const WM_APP: u32 = 0x8000;
const WM_TRAY: u32 = WM_APP + 10;
const WM_DEVTOOLS: u32 = WM_APP + 11;
const WM_P_SCANNED: u32 = WM_APP + 20;
const WM_P_DONE: u32 = WM_APP + 21;
const NIN_BALLOONUSERCLICK: u32 = 0x405;

const NIM_ADD: u32 = 0;
const NIM_MODIFY: u32 = 1;
const NIM_DELETE: u32 = 2;
const NIF_MESSAGE: u32 = 1;
const NIF_ICON: u32 = 2;
const NIF_TIP: u32 = 4;
const NIF_INFO: u32 = 0x10;
const NIIF_INFO: u32 = 1;
const NIIF_WARNING: u32 = 2;

const MF_STRING: u32 = 0;
const MF_SEPARATOR: u32 = 0x800;
const MF_CHECKED: u32 = 8;
const MF_GRAYED: u32 = 1;
const TPM_RIGHTBUTTON: u32 = 2;
const TPM_RETURNCMD: u32 = 0x100;
const TPM_NONOTIFY: u32 = 0x80;
const ERROR_ALREADY_EXISTS: u32 = 183;
const WS_POPUP: u32 = 0x8000_0000;
const WS_CHILD: u32 = 0x4000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_EX_TOPMOST: u32 = 0x8;
const WS_EX_TOOLWINDOW: u32 = 0x80;
const BS_OWNERDRAW: u32 = 0xB;
const CS_DROPSHADOW: u32 = 0x0002_0000;
const SW_SHOW: i32 = 5;
const SPI_GETWORKAREA: u32 = 0x30;
const SRCCOPY: u32 = 0x00CC_0020;

const CMD_PANEL: usize = 1;
const CMD_OPEN: usize = 2;
const CMD_AUTOSTART: usize = 3;
const CMD_EXIT: usize = 4;

// Not 1/2: those are IDOK/IDCANCEL.
const ID_P_CLOSE: u16 = 101;
const ID_P_OPTIMIZE: u16 = 102;
const ID_P_OPEN: u16 = 103;
const TIMER_TICK: usize = 1;
const TIMER_P_SPIN: usize = 2;

const TICK_MS: u32 = 5_000;
/// Critical for 2 minutes before warning.
const CRITICAL_TICKS: u32 = 24;
/// Check for idle dev tools every 10 minutes.
const DEVTOOL_TICKS: u64 = 120;
const ALERT_COOLDOWN_SECS: u64 = 30 * 60;
const FT_SEC: u64 = crate::util::FT_PER_SEC;

pub const TRAY_CLASS: &str = "OneClickOptimizerTray";
const POPUP_CLASS: &str = "OneClickOptimizerPanel";
const POPUP_W: i32 = 360;
const POPUP_H: i32 = 520;

#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(hwnd: HWND, attr: u32, value: *const std::ffi::c_void, size: u32) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetTickCount64() -> u64;
}

struct State {
    hwnd: HWND,
    level: Option<Level>,
    icon: HICON,
    critical_ticks: u32,
    ticks: u64,
    last_alert: u64,
    last_devtool_sig: String,
    devtool_check_running: bool,
    taskbar_created: u32,
}

thread_local! {
    static ST: RefCell<Option<State>> = const { RefCell::new(None) };
}

static DEVTOOL_RESULT: Mutex<Option<(String, usize, u64, String)>> = Mutex::new(None);

/// Appends one line to %LOCALAPPDATA%\OneClickOptimizer\agent.log (kept under ~64 KB).
fn log(event: &str) {
    use std::io::Write;
    let path = crate::util::data_dir().join("agent.log");
    if std::fs::metadata(&path).map_or(false, |m| m.len() > 64 * 1024) {
        let _ = std::fs::rename(&path, path.with_extension("old.log"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{}  {event}", sys::mem::fmt_local(sys::mem::now_filetime()));
    }
}

/// Timer period; OCO_TICK_MS shortens it for automated tests.
fn tick_ms() -> u32 {
    std::env::var("OCO_TICK_MS").ok().and_then(|v| v.parse().ok()).filter(|v| *v >= 50).unwrap_or(TICK_MS)
}

fn copy_into(dst: &mut [u16], s: &str) {
    let w: Vec<u16> = s.encode_utf16().take(dst.len() - 1).collect();
    dst[..w.len()].copy_from_slice(&w);
    dst[w.len()] = 0;
}

fn base_nid(hwnd: HWND) -> NOTIFYICONDATAW {
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = 1;
    nid
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
        hIcon: null_mut(),
        hCursor: LoadCursorW(null_mut(), IDC_ARROW),
        hbrBackground: null_mut(),
        lpszMenuName: null(),
        lpszClassName: name.as_ptr(),
        hIconSm: null_mut(),
    };
    RegisterClassExW(&wc);
}

pub fn run() {
    unsafe {
        SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_SYSTEM_AWARE);
        CreateMutexW(null(), 0, wide("Local\\OneClickOptimizer.Tray").as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            return; // already running
        }
        startup();
        register(TRAY_CLASS, wndproc, 0);
        register(POPUP_CLASS, popup_proc, CS_DROPSHADOW);
        // Hidden top-level window (never shown) to receive tray and timer messages.
        let hwnd = CreateWindowExW(0, wide(TRAY_CLASS).as_ptr(), wide("OneClick Optimizer tray").as_ptr(), 0, 0, 0, 0, 0, null_mut(), null_mut(), GetModuleHandleW(null()), null());
        if hwnd.is_null() {
            return;
        }
        ST.with(|s| {
            *s.borrow_mut() = Some(State {
                hwnd,
                level: None,
                icon: null_mut(),
                critical_ticks: 0,
                ticks: 0,
                last_alert: 0,
                last_devtool_sig: String::new(),
                devtool_check_running: false,
                taskbar_created: RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()),
            })
        });
        migrate_autostart();
        add_icon(hwnd);
        log(&format!("tray watch started (pid {}, every {} ms)", std::process::id(), tick_ms()));
        tick();
        SetTimer(hwnd, TIMER_TICK, tick_ms(), None);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// The app used to be called RAM Doctor: move its "start with Windows" entry over.
fn migrate_autostart() {
    if sys::startup::remove_legacy_autostart() {
        if let Ok(exe) = std::env::current_exe() {
            let _ = sys::startup::set_agent_autostart(true, &exe.to_string_lossy());
        }
    }
}

unsafe fn add_icon(hwnd: HWND) {
    let mut nid = base_nid(hwnd);
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    let icon = icon::tray_icon(Level::Normal);
    nid.hIcon = icon;
    copy_into(&mut nid.szTip, "OneClick Optimizer: checking memory\u{2026}");
    Shell_NotifyIconW(NIM_ADD, &nid);
    ST.with(|s| {
        if let Some(st) = s.borrow_mut().as_mut() {
            if !st.icon.is_null() {
                DestroyIcon(st.icon);
            }
            st.icon = icon;
            st.level = Some(Level::Normal);
        }
    });
}

unsafe fn balloon(hwnd: HWND, title: &str, text: &str, warning: bool) {
    let mut nid = base_nid(hwnd);
    nid.uFlags = NIF_INFO;
    copy_into(&mut nid.szInfoTitle, title);
    copy_into(&mut nid.szInfo, text);
    nid.dwInfoFlags = if warning { NIIF_WARNING } else { NIIF_INFO };
    Shell_NotifyIconW(NIM_MODIFY, &nid);
}

/// Every 5 s: one cheap memory read, icon/tooltip update, and alerts.
fn tick() {
    let mem = sys::mem::read_mem();
    let (ratio, level) = analyze::pressure(&mem);
    let now = sys::mem::now_filetime();
    let Some((hwnd, changed, alert, devtools)) = ST.with(|s| {
        let mut b = s.borrow_mut();
        let st = b.as_mut()?;
        st.ticks += 1;
        let changed = st.level != Some(level);
        st.level = Some(level);
        st.critical_ticks = if level == Level::Critical { st.critical_ticks + 1 } else { 0 };
        let cooled = now.saturating_sub(st.last_alert) > ALERT_COOLDOWN_SECS * FT_SEC;
        let alert = st.critical_ticks >= CRITICAL_TICKS && cooled;
        if alert {
            st.last_alert = now;
        }
        let devtools = st.ticks % DEVTOOL_TICKS == 0 && !st.devtool_check_running;
        if devtools {
            st.devtool_check_running = true;
        }
        Some((st.hwnd, changed, alert, devtools))
    }) else {
        return;
    };

    unsafe {
        let mut nid = base_nid(hwnd);
        nid.uFlags = NIF_TIP;
        copy_into(
            &mut nid.szTip,
            &format!(
                "OneClick Optimizer: memory {}\n{} used of {} RAM ({:.0}%). Click for one-click Optimise.",
                level.label().to_lowercase(),
                fmt_bytes(mem.commit_used),
                fmt_bytes(mem.total_phys),
                ratio * 100.0
            ),
        );
        if changed {
            let icon = icon::tray_icon(level);
            nid.uFlags |= NIF_ICON;
            nid.hIcon = icon;
            Shell_NotifyIconW(NIM_MODIFY, &nid);
            ST.with(|s| {
                if let Some(st) = s.borrow_mut().as_mut() {
                    if !st.icon.is_null() {
                        DestroyIcon(st.icon);
                    }
                    st.icon = icon;
                }
            });
        } else {
            Shell_NotifyIconW(NIM_MODIFY, &nid);
        }
    }

    if alert {
        let procs = sys::procs::list_processes(0, false);
        let top: Vec<String> = analyze::group_apps(&procs, std::process::id())
            .iter()
            .take(3)
            .map(|g| format!("{} {}", g.name, fmt_bytes(g.private_bytes)))
            .collect();
        let text = format!("{}. Biggest: {}. Click the tray icon for one-click Optimise.", report::headline(&mem), top.join(", "));
        log(&format!("alert: critical memory. {text}"));
        unsafe { balloon(hwnd, "Memory is critically low", &text, true) };
    }

    if devtools {
        // CPU sampling takes a second; keep the tray responsive by doing it off-thread.
        let target = hwnd as usize;
        std::thread::spawn(move || {
            let snap = Snapshot { procs: sys::procs::list_processes(1000, true), now: sys::mem::now_filetime(), self_pid: std::process::id(), ..Default::default() };
            let idle: Vec<_> = analyze::analyze(&snap).into_iter().filter(|r| r.category == Category::DevTool && r.preselect).collect();
            let sig: String = idle.iter().map(|r| r.title.as_str()).collect::<Vec<_>>().join("|");
            let bytes: u64 = idle.iter().map(|r| r.saves_bytes).sum();
            let names = idle
                .iter()
                .take(2)
                .map(|r| match &r.action {
                    crate::actions::Action::KillProcesses { label, .. } => label.clone(),
                    _ => r.title.clone(),
                })
                .collect::<Vec<_>>()
                .join("; ");
            *DEVTOOL_RESULT.lock().unwrap_or_else(|e| e.into_inner()) = Some((sig, idle.len(), bytes, names));
            unsafe { PostMessageW(target as HWND, WM_DEVTOOLS, 0, 0) };
        });
    }
}

fn on_devtools_result() {
    let Some((sig, count, bytes, names)) = DEVTOOL_RESULT.lock().unwrap_or_else(|e| e.into_inner()).take() else {
        return;
    };
    let notify = ST.with(|s| {
        let mut b = s.borrow_mut();
        let st = b.as_mut()?;
        st.devtool_check_running = false;
        let fresh = count > 0 && sig != st.last_devtool_sig;
        st.last_devtool_sig = sig;
        fresh.then_some(st.hwnd)
    });
    log(&format!("dev-tool check: {count} idle, {}", fmt_bytes(bytes)));
    if let Some(hwnd) = notify {
        let title = format!("{count} idle dev tool(s) holding {}", fmt_bytes(bytes));
        log(&format!("alert: {title}. {names}"));
        unsafe { balloon(hwnd, &title, &format!("{names}. Click the tray icon to free it in one click."), false) };
    }
}

fn open_app() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::process::Command::new(exe).spawn();
    }
}

unsafe fn show_menu(hwnd: HWND) {
    let menu = CreatePopupMenu();
    let autostart = sys::startup::agent_autostart();
    let status = ST.with(|s| s.borrow().as_ref().and_then(|st| st.level)).map_or("Checking\u{2026}", |l| l.label());
    AppendMenuW(menu, MF_STRING | MF_GRAYED, 0, wide(&format!("Memory: {status}")).as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    AppendMenuW(menu, MF_STRING, CMD_PANEL, wide("Quick optimise\u{2026}").as_ptr());
    AppendMenuW(menu, MF_STRING, CMD_OPEN, wide(&format!("Open {APP_NAME}")).as_ptr());
    AppendMenuW(menu, MF_STRING | if autostart { MF_CHECKED } else { 0 }, CMD_AUTOSTART, wide("Start with Windows").as_ptr());
    AppendMenuW(menu, MF_SEPARATOR, 0, null());
    AppendMenuW(menu, MF_STRING, CMD_EXIT, wide("Exit tray watch").as_ptr());
    let mut pt = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    SetForegroundWindow(hwnd); // so the menu closes when clicking elsewhere
    let cmd = TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY, pt.x, pt.y, 0, hwnd, null()) as usize;
    DestroyMenu(menu);
    match cmd {
        CMD_PANEL => open_panel(),
        CMD_OPEN => open_app(),
        CMD_AUTOSTART => {
            if let Ok(exe) = std::env::current_exe() {
                let _ = sys::startup::set_agent_autostart(!autostart, &exe.to_string_lossy());
            }
        }
        CMD_EXIT => {
            DestroyWindow(hwnd);
        }
        _ => {}
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let taskbar_created = ST.with(|s| s.try_borrow().ok().and_then(|b| b.as_ref().map(|st| st.taskbar_created)));
    match msg {
        WM_TIMER => {
            tick();
            0
        }
        WM_TRAY => {
            match (lparam as u32) & 0xFFFF {
                WM_LBUTTONUP => toggle_panel(),
                WM_LBUTTONDBLCLK | NIN_BALLOONUSERCLICK => open_app(),
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
                _ => {}
            }
            0
        }
        WM_DEVTOOLS => {
            on_devtools_result();
            0
        }
        WM_DESTROY => {
            let nid = base_nid(hwnd);
            Shell_NotifyIconW(NIM_DELETE, &nid);
            log("tray watch stopped");
            PostQuitMessage(0);
            0
        }
        _ if Some(msg) == taskbar_created && msg != 0 => {
            add_icon(hwnd); // Explorer restarted: put the icon back
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

// =============================================================== quick panel

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PState {
    Checking,
    Ready,
    Nothing,
    Working,
    Done,
}

#[derive(Clone, Copy)]
struct Panel {
    hwnd: HWND,
    close: HWND,
    optimize: HWND,
    open: HWND,
    dpi: i32,
    f: Fonts,
    logo: HICON,
}

#[derive(Default)]
struct PData {
    mem: MemInfo,
    top: Vec<(String, u64)>,
    scan: Option<ScanResult>,
    ops: Vec<Op>,
    lines: Vec<String>,
    done: Option<RunSummary>,
}

static PDATA: Mutex<Option<PData>> = Mutex::new(None);

thread_local! {
    static PANEL: Cell<Option<Panel>> = const { Cell::new(None) };
    static PSTATE: Cell<PState> = const { Cell::new(PState::Checking) };
    static PSPIN: Cell<f32> = const { Cell::new(0.0) };
    static PSTEP: Cell<usize> = const { Cell::new(0) };
    static CLOSED_AT: Cell<u64> = const { Cell::new(0) };
}

fn with_pdata<R>(f: impl FnOnce(&mut PData) -> R) -> R {
    let mut g = PDATA.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(PData::default))
}

fn panel() -> Option<Panel> {
    PANEL.with(Cell::get)
}

fn ps(p: &Panel, v: i32) -> i32 {
    v * p.dpi / 96
}

fn toggle_panel() {
    match panel() {
        Some(p) => unsafe {
            if PSTATE.with(Cell::get) != PState::Working {
                DestroyWindow(p.hwnd);
            }
        },
        None => {
            // A click on the tray icon first deactivates (closes) the panel;
            // don't reopen it from that same click.
            let since = unsafe { GetTickCount64() }.saturating_sub(CLOSED_AT.with(Cell::get));
            if since > 400 {
                open_panel();
            }
        }
    }
}

/// Opens the quick panel above the tray (bottom-right of the work area).
pub fn open_panel() {
    if panel().is_some() {
        return;
    }
    unsafe {
        let dpi = GetDpiForSystem() as i32;
        let (w, h) = (POPUP_W * dpi / 96, POPUP_H * dpi / 96);
        let mut work = rect(0, 0, 1280, 720);
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut work as *mut RECT as *mut _, 0);
        let offscreen = std::env::var_os("OCO_OFFSCREEN").is_some();
        let (x, y) = if offscreen { (-10_000, -10_000) } else { (work.right - w - 12 * dpi / 96, work.bottom - h - 12 * dpi / 96) };
        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
            wide(POPUP_CLASS).as_ptr(),
            wide(APP_NAME).as_ptr(),
            WS_POPUP,
            x,
            y,
            w,
            h,
            null_mut(),
            null_mut(),
            GetModuleHandleW(null()),
            null(),
        );
        if hwnd.is_null() {
            return;
        }
        // Windows 11 rounded corners.
        let round: u32 = 2;
        DwmSetWindowAttribute(hwnd, 33, &round as *const u32 as *const _, 4);
        let mk = |id: u16| {
            let b = CreateWindowExW(0, wide("BUTTON").as_ptr(), wide("").as_ptr(), WS_CHILD | WS_VISIBLE | BS_OWNERDRAW, 0, 0, 10, 10, hwnd, id as usize as _, GetModuleHandleW(null()), null());
            add_hover(b);
            b
        };
        let p = Panel {
            hwnd,
            close: mk(ID_P_CLOSE),
            optimize: mk(ID_P_OPTIMIZE),
            open: mk(ID_P_OPEN),
            dpi,
            f: Fonts::new(dpi),
            logo: icon::app_icon(24 * dpi / 96),
        };
        PANEL.with(|c| c.set(Some(p)));
        let s = |v: i32| v * dpi / 96;
        windows_sys::Win32::UI::WindowsAndMessaging::MoveWindow(p.close, w - s(46), s(10), s(34), s(34), 1);
        windows_sys::Win32::UI::WindowsAndMessaging::MoveWindow(p.optimize, s(16), h - s(124), w - s(32), s(64), 1);
        windows_sys::Win32::UI::WindowsAndMessaging::MoveWindow(p.open, s(16), h - s(52), w - s(32), s(36), 1);

        // Instant numbers now; the full check runs in the background.
        let mem = sys::mem::read_mem();
        let procs = sys::procs::list_processes(0, false);
        let top: Vec<(String, u64)> = analyze::group_apps(&procs, std::process::id()).iter().take(3).map(|g| (g.name.clone(), g.private_bytes)).collect();
        with_pdata(|d| *d = PData { mem, top, ..Default::default() });
        PSTATE.with(|c| c.set(PState::Checking));
        PSTEP.with(|c| c.set(0));
        EnableWindow(p.optimize, 0);
        SetTimer(hwnd, TIMER_P_SPIN, 40, None);
        if offscreen {
            ShowWindow(hwnd, 4); // SW_SHOWNOACTIVATE: tests never take keyboard focus
        } else {
            ShowWindow(hwnd, SW_SHOW);
            SetForegroundWindow(hwnd);
        }
        log("quick panel opened");

        let target = hwnd as usize;
        std::thread::spawn(move || {
            // Progress goes to the panel's thread as WM_TIMER with lparam = step + 1.
            let scan = optimize::deep_scan(&|step| {
                PostMessageW(target as HWND, WM_TIMER, TIMER_P_SPIN, step as isize + 1);
            });
            let ops = optimize::default_ops(&scan);
            let lines = optimize::plan_lines(&scan, &ops);
            with_pdata(|d| {
                d.mem = scan.snap.mem.clone();
                d.top = analyze::group_apps(&scan.snap.procs, std::process::id()).iter().take(3).map(|g| (g.name.clone(), g.private_bytes)).collect();
                d.ops = ops;
                d.lines = lines;
                d.scan = Some(scan);
            });
            PostMessageW(target as HWND, WM_P_SCANNED, 0, 0);
        });
    }
}

unsafe fn panel_paint(p: &Panel, hdc: HDC, w: i32, h: i32) {
    let s = |v: i32| ps(p, v);
    // Hairline border (the window has no frame), then the white body.
    fill(hdc, &rect(0, 0, w, h), BORDER);
    fill(hdc, &rect(1, 1, w - 1, h - 1), CARD);
    DrawIconEx(hdc, s(16), s(16), p.logo, s(24), s(24), 0, null_mut(), 3);
    text(hdc, APP_NAME, rect(s(48), s(12), w - s(52), s(44)), p.f.bold, INK, DT_SINGLELINE | DT_VCENTER);

    let state = PSTATE.with(Cell::get);
    let (mem, top, lines, done) = with_pdata(|d| (d.mem.clone(), d.top.clone(), d.lines.clone(), d.done.clone()));
    let (ratio, level) = analyze::pressure(&mem);
    let color = level_color(level);

    // Memory ring.
    let gfx = Gfx::new(hdc);
    let r = s(38) as f32;
    let (cx, cy) = (s(20) as f32 + r, s(104) as f32);
    gfx.arc(cx, cy, r, s(10) as f32, 0.0, 360.0, TRACK);
    gfx.arc(cx, cy, r, s(10) as f32, -90.0, ((ratio / 2.0).clamp(0.02, 1.0) * 360.0) as f32, color);
    drop(gfx);
    let ring = rect((cx - r) as i32, (cy - r) as i32, (cx + r) as i32, (cy + r) as i32);
    text(hdc, &format!("{:.0}%", ratio * 100.0), ring, p.f.section, color, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    let tx = ring.right + s(18);
    let title = match level {
        Level::Critical => "Memory is overloaded",
        Level::High => "Memory is getting tight",
        Level::Normal => "Memory looks healthy",
    };
    text(hdc, title, rect(tx, s(70), w - s(12), s(96)), p.f.section, color, DT_SINGLELINE | DT_END_ELLIPSIS);
    text(hdc, &format!("{} used on {} of RAM", fmt_bytes(mem.commit_used), fmt_bytes(mem.total_phys)), rect(tx, s(98), w - s(12), s(118)), p.f.small, INK, DT_SINGLELINE | DT_END_ELLIPSIS);
    text(hdc, &format!("Free right now: {}", fmt_bytes(mem.avail_phys)), rect(tx, s(118), w - s(12), s(138)), p.f.small, MUTED, DT_SINGLELINE);

    // Biggest apps.
    text(hdc, "USING THE MOST MEMORY", rect(s(18), s(160), w - s(16), s(180)), p.f.small_bold, MUTED, DT_SINGLELINE);
    for (i, (name, bytes)) in top.iter().enumerate() {
        let y = s(182) + i as i32 * s(24);
        text(hdc, name, rect(s(18), y, w - s(110), y + s(22)), p.f.body, INK, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
        text(hdc, &fmt_bytes(*bytes), rect(w - s(110), y, w - s(18), y + s(22)), p.f.bold, ACCENT_DOWN, DT_SINGLELINE | DT_VCENTER | DT_RIGHT);
    }

    // Plan / progress / result box.
    let bx = rect(s(16), s(262), w - s(16), h - s(136));
    let gfx = Gfx::new(hdc);
    gfx.fill_round(&bx, s(12) as f32, BG, None);
    let inner_l = bx.left + s(14);
    match state {
        PState::Checking | PState::Working => {
            let rr = s(16) as f32;
            let (sx, sy) = ((inner_l as f32) + rr, (bx.top + s(34)) as f32);
            gfx.arc(sx, sy, rr, s(4) as f32, 0.0, 360.0, TRACK);
            gfx.arc(sx, sy, rr, s(4) as f32, PSPIN.with(Cell::get), 80.0, ACCENT);
            drop(gfx);
            let (head, steps) = if state == PState::Checking { ("Checking what can be fixed\u{2026}", &optimize::SCAN_STEPS) } else { ("Optimising\u{2026}", &optimize::RUN_STEPS) };
            text(hdc, head, rect(inner_l + s(44), bx.top + s(18), bx.right - s(10), bx.top + s(50)), p.f.bold, INK, DT_SINGLELINE | DT_VCENTER);
            let step = PSTEP.with(Cell::get).min(3);
            text(hdc, steps[step], rect(inner_l, bx.top + s(62), bx.right - s(10), bx.top + s(84)), p.f.small, MUTED, DT_SINGLELINE | DT_END_ELLIPSIS);
        }
        PState::Ready | PState::Nothing => {
            drop(gfx);
            if state == PState::Nothing {
                text(hdc, "\u{E73E}", rect(inner_l, bx.top + s(14), inner_l + s(26), bx.top + s(40)), p.f.icons, GOOD, DT_SINGLELINE | DT_VCENTER);
                text(hdc, "Nothing to fix right now", rect(inner_l + s(30), bx.top + s(14), bx.right - s(10), bx.top + s(40)), p.f.bold, INK, DT_SINGLELINE | DT_VCENTER);
                text(hdc, "Your PC is in good shape. Open the full app for details.", rect(inner_l, bx.top + s(46), bx.right - s(10), bx.bottom - s(8)), p.f.small, MUTED, DT_WORDBREAK);
            } else {
                text(hdc, "ONE CLICK WILL", rect(inner_l, bx.top + s(10), bx.right, bx.top + s(30)), p.f.small_bold, MUTED, DT_SINGLELINE);
                for (i, line) in lines.iter().take(4).enumerate() {
                    let y = bx.top + s(34) + i as i32 * s(24);
                    text(hdc, "\u{E73E}", rect(inner_l, y, inner_l + s(22), y + s(22)), p.f.icons, ACCENT, DT_SINGLELINE | DT_VCENTER);
                    text(hdc, line, rect(inner_l + s(26), y, bx.right - s(8), y + s(22)), p.f.body, INK, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
                }
            }
        }
        PState::Done => {
            drop(gfx);
            let summary = done.unwrap_or_default();
            let head = if summary.dry_run { "Test run \u{2014} nothing was changed" } else { "Done! Your PC is optimised" };
            text(hdc, "\u{E73E}", rect(inner_l, bx.top + s(12), inner_l + s(24), bx.top + s(38)), p.f.icons, GOOD, DT_SINGLELINE | DT_VCENTER);
            text(hdc, head, rect(inner_l + s(30), bx.top + s(12), bx.right - s(8), bx.top + s(38)), p.f.bold, GOOD, DT_SINGLELINE | DT_VCENTER);
            for (i, line) in summary.lines().iter().take(3).enumerate() {
                let y = bx.top + s(44) + i as i32 * s(22);
                text(hdc, line, rect(inner_l, y, bx.right - s(8), y + s(22)), p.f.small, INK, DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS);
            }
        }
    }
}

unsafe fn panel_on_paint(p: &Panel) {
    let mut ps_: PAINTSTRUCT = std::mem::zeroed();
    let hdc = BeginPaint(p.hwnd, &mut ps_);
    let mut rc = rect(0, 0, 0, 0);
    GetClientRect(p.hwnd, &mut rc);
    let (w, h) = (rc.right.max(1), rc.bottom.max(1));
    let mem_dc = CreateCompatibleDC(hdc);
    let bmp = CreateCompatibleBitmap(hdc, w, h);
    let old = SelectObject(mem_dc, bmp);
    panel_paint(p, mem_dc, w, h);
    BitBlt(hdc, 0, 0, w, h, mem_dc, 0, 0, SRCCOPY);
    SelectObject(mem_dc, old);
    DeleteObject(bmp);
    DeleteDC(mem_dc);
    EndPaint(p.hwnd, &ps_);
}

unsafe fn panel_draw_item(p: &Panel, d: &DrawItem) {
    let s = |v: i32| ps(p, v);
    match d.ctl_id as u16 {
        ID_P_CLOSE => {
            fill(d.hdc, &d.rc, CARD);
            if hovered(d.hwnd_item) {
                Gfx::new(d.hdc).fill_round(&d.rc, s(8) as f32, BG, None);
            }
            text(d.hdc, "\u{E711}", d.rc, p.f.icons, MUTED, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
        ID_P_OPTIMIZE => {
            let state = PSTATE.with(Cell::get);
            let (fix, junk) = with_pdata(|d| d.scan.as_ref().map(|r| (r.fix_bytes(), r.junk_bytes())).unwrap_or((0, 0)));
            let (label, sub) = match state {
                PState::Checking => ("Checking\u{2026}".to_string(), String::new()),
                PState::Working => ("Optimising\u{2026}".to_string(), String::new()),
                PState::Done => ("Close".to_string(), String::new()),
                PState::Nothing => ("All good".to_string(), String::new()),
                PState::Ready => ("Optimise now".to_string(), optimize::frees_text(fix, junk).unwrap_or_default()),
            };
            draw_primary(d, CARD, &label, &sub, &p.f, Some(if state == PState::Done { '\u{E73E}' } else { '\u{E945}' }), p.dpi);
        }
        ID_P_OPEN => {
            fill(d.hdc, &d.rc, CARD);
            let fg = if hovered(d.hwnd_item) { ACCENT_DOWN } else { ACCENT };
            text(d.hdc, &format!("Open {APP_NAME}  \u{2192}"), d.rc, p.f.bold, fg, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
        }
        _ => {}
    }
}

unsafe fn panel_optimize(p: &Panel) {
    match PSTATE.with(Cell::get) {
        PState::Done | PState::Nothing => {
            DestroyWindow(p.hwnd);
            return;
        }
        PState::Ready => {}
        _ => return,
    }
    let (ops, junk) = with_pdata(|d| (d.ops.clone(), d.scan.as_ref().map(|s| s.junk_bytes()).unwrap_or(0)));
    if ops.is_empty() {
        return;
    }
    PSTATE.with(|c| c.set(PState::Working));
    PSTEP.with(|c| c.set(0));
    EnableWindow(p.optimize, 0);
    SetTimer(p.hwnd, TIMER_P_SPIN, 40, None);
    InvalidateRect(p.hwnd, null(), 0);
    log(&format!("quick panel: optimise ({} changes)", ops.len()));
    let target = p.hwnd as usize;
    std::thread::spawn(move || {
        let summary = optimize::run(ops, junk, &|step| unsafe {
            PostMessageW(target as HWND, WM_TIMER, TIMER_P_SPIN, step as isize + 1);
        });
        log(&format!("quick panel: done. {}", summary.lines().join("; ")));
        with_pdata(|d| d.done = Some(summary));
        unsafe { PostMessageW(target as HWND, WM_P_DONE, 0, 0) };
    });
}

unsafe extern "system" fn popup_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            match panel() {
                Some(p) if p.hwnd == hwnd => panel_on_paint(&p),
                _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
            }
            0
        }
        WM_DRAWITEM => {
            if let Some(p) = panel() {
                panel_draw_item(&p, &*(lparam as *const DrawItem));
            }
            1
        }
        WM_COMMAND => {
            if let Some(p) = panel() {
                match (wparam & 0xFFFF) as u16 {
                    ID_P_CLOSE => {
                        if PSTATE.with(Cell::get) != PState::Working {
                            DestroyWindow(hwnd);
                        }
                    }
                    ID_P_OPTIMIZE => panel_optimize(&p),
                    ID_P_OPEN => {
                        open_app();
                        if PSTATE.with(Cell::get) != PState::Working {
                            DestroyWindow(hwnd);
                        }
                    }
                    _ => {}
                }
            }
            0
        }
        WM_TIMER => {
            if wparam == TIMER_P_SPIN {
                if lparam > 0 {
                    // Progress update from a worker thread (step + 1).
                    PSTEP.with(|c| c.set(lparam as usize - 1));
                }
                PSPIN.with(|c| c.set((c.get() + 10.0) % 360.0));
                if let Some(p) = panel() {
                    let r = rect(0, ps(&p, 262), ps(&p, POPUP_W), ps(&p, POPUP_H) - ps(&p, 136));
                    InvalidateRect(hwnd, &r, 0);
                }
            }
            0
        }
        WM_P_SCANNED => {
            KillTimer(hwnd, TIMER_P_SPIN);
            let any = with_pdata(|d| !d.ops.is_empty());
            PSTATE.with(|c| c.set(if any { PState::Ready } else { PState::Nothing }));
            if let Some(p) = panel() {
                EnableWindow(p.optimize, 1);
                InvalidateRect(p.optimize, null(), 0);
            }
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_P_DONE => {
            KillTimer(hwnd, TIMER_P_SPIN);
            PSTATE.with(|c| c.set(PState::Done));
            with_pdata(|d| d.mem = sys::mem::read_mem());
            if let Some(p) = panel() {
                EnableWindow(p.optimize, 1);
                InvalidateRect(p.optimize, null(), 0);
            }
            InvalidateRect(hwnd, null(), 0);
            0
        }
        WM_ACTIVATE => {
            // Click outside closes the panel (but never while it is working).
            let offscreen = std::env::var_os("OCO_OFFSCREEN").is_some();
            if wparam & 0xFFFF == 0 && !offscreen && PSTATE.with(Cell::get) != PState::Working {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_DESTROY => {
            KillTimer(hwnd, TIMER_P_SPIN);
            PANEL.with(|c| c.set(None));
            CLOSED_AT.with(|c| c.set(GetTickCount64()));
            with_pdata(|d| *d = PData::default());
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
